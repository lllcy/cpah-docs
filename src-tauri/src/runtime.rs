use crate::converter::{
    LocalConversion, StagedMinerUPart, convert_locally, copy_markdown, default_engine, is_enabled,
    is_markdown, is_pdf, is_supported, output_path, remove_generated_output, write_artifact,
    write_multipart_artifact, write_staged_mineru_artifact,
};
use crate::mineru::MinerUClient;
use crate::models::{
    ConversionEngine, DeletePolicy, JobStatus, MinerUPartMode, MinerUPartRecord, TaskRecord,
    WatchProfile,
};
use crate::pdf_split::{PdfPlan, plan_pdf, recreate_physical_part};
use crate::state::{AppState, ProfileCancelled, ProfileRuntimeControl, is_profile_cancelled};
use crate::tag_runtime::TagRuntimeMessage;
use anyhow::{Context, Result};
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tokio::sync::{Mutex, Semaphore, mpsc};
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Debug)]
pub enum RuntimeMessage {
    Reload,
    PriorityFile {
        profile_id: String,
        relative_path: String,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    ProcessQueued,
    Path {
        path: PathBuf,
        force: bool,
    },
    Retry {
        task_id: String,
    },
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
    let mut priority_interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            Some(message) = receiver.recv() => {
                match message {
                    RuntimeMessage::PriorityFile { profile_id, relative_path, reply } => {
                        let result = queue_priority_file(&state, &active, &profile_id, &relative_path).await;
                        let _ = reply.send(result.map_err(|error| format!("{error:#}")));
                        if let Err(error) = pump_priority_files(&state, &mineru, &active, &semaphore).await {
                            tracing::error!(error = %format!("{error:#}"), "priority conversion failed");
                        }
                    }
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
                            && let Err(error) = reconcile_missing_sources(&state, &active).await
                        {
                            tracing::error!(error = %format!("{error:#}"), "source reconciliation failed");
                        }
                        if let Err(error) = scan_all(&state, &mineru, &active, &semaphore).await {
                            tracing::error!(error = %format!("{error:#}"), "folder scan after reconciliation failed");
                        }
                    }
                }
            }
            _ = priority_interval.tick() => {
                if let Err(error) = pump_priority_files(&state, &mineru, &active, &semaphore).await {
                    tracing::error!(error = %format!("{error:#}"), "priority conversion failed");
                }
            }
            _ = interval.tick() => {
                if !state.is_monitoring_paused()
                    && let Err(error) = reconcile_missing_sources(&state, &active).await
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

async fn queue_priority_file(
    state: &AppState,
    active: &SharedActivePaths,
    profile_id: &str,
    relative: &str,
) -> Result<()> {
    let settings = state.settings.read().await.clone();
    let (profile, source) = crate::file_actions::source_file(&settings, profile_id, relative)?;
    let control = state
        .profile_control(&profile.id)
        .context(ProfileCancelled)?;
    let _operation_permit = control.write_permit().await?;
    let existing = state.storage.find_by_source(&source.to_string_lossy())?;
    let output = output_path(&profile, &source)?;
    if state
        .storage
        .find_tag_job_by_path(&output)?
        .is_some_and(|job| {
            matches!(
                job.status,
                crate::models::TagJobStatus::Reading | crate::models::TagJobStatus::Writing
            )
        })
    {
        anyhow::bail!("此文件正在分类，请等待完成后再转换");
    }
    if let Some(task) = &existing {
        if state.conversion_priority.contains(&task.id) {
            return state.resume_file_queue(false).await;
        }
        if matches!(
            task.status,
            JobStatus::Converting
                | JobStatus::Uploading
                | JobStatus::Processing
                | JobStatus::Downloading
        ) || (active.lock().await.running.contains(&source) && task.status != JobStatus::Queued)
            || state
                .storage
                .list_mineru_parts_for_parent(&task.id)?
                .iter()
                .any(|part| {
                    matches!(
                        part.status,
                        JobStatus::Uploading | JobStatus::Processing | JobStatus::Downloading
                    )
                })
        {
            anyhow::bail!("此文件正在转换，请等待完成");
        }
    }
    let task = match existing {
        Some(task) if task.status != JobStatus::Completed => task,
        _ => {
            let metadata = std::fs::metadata(&source)?;
            let queued = state
                .storage
                .queue_task(
                    &profile,
                    &source,
                    source.strip_prefix(&profile.input_dir)?,
                    metadata.len(),
                    metadata.modified()?.duration_since(UNIX_EPOCH)?.as_millis() as i64,
                    default_engine(&source).context("此格式不支持转换")?,
                    &output,
                    true,
                )?
                .context("无法创建转换任务")?;
            // An explicit reconversion must not merely merge an old completed PDF.
            state.storage.delete_mineru_parts(&queued.id)?;
            queued
        }
    };
    if task.status == JobStatus::Failed {
        state
            .storage
            .set_status(&task.id, JobStatus::Queued, None)?;
    }
    state.conversion_priority.push(task.id.clone());
    if let Err(error) = state.resume_file_queue(false).await {
        state.conversion_priority.remove(&task.id);
        return Err(error);
    }
    Ok(())
}

async fn pump_priority_files(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
) -> Result<()> {
    if state.is_paused() || state.conversion_priority.front().is_none() {
        return Ok(());
    }
    while let Some((id, started)) = state.conversion_priority.front() {
        let Some(task) = state.storage.get_task(&id)? else {
            state.conversion_priority.remove(&id);
            continue;
        };
        let source = PathBuf::from(&task.source_path);
        let settings = state.settings.read().await.clone();
        let valid = settings
            .profiles
            .iter()
            .find(|profile| profile.id == task.profile_id)
            .filter(|profile| profile.enabled)
            .is_some_and(|profile| {
                crate::file_browser::checked_file(
                    Path::new(&profile.input_dir),
                    &task.relative_path,
                )
                .is_ok_and(|checked| checked == source)
            })
            && is_enabled(&source, &settings.enabled_extensions);
        if !valid {
            state.storage.set_status(
                &id,
                JobStatus::Failed,
                Some("文件不可用、目录已停用或格式已关闭"),
            )?;
            state.conversion_priority.remove(&id);
            continue;
        }
        if started
            && matches!(
                task.status,
                JobStatus::Completed | JobStatus::Failed | JobStatus::WaitingMineru
            )
            && !active.lock().await.running.contains(&source)
        {
            state.conversion_priority.remove(&id);
            continue;
        }
        if !active.lock().await.running.contains(&source) {
            if !started || matches!(task.status, JobStatus::Queued | JobStatus::WaitingStable) {
                state.conversion_priority.mark_started(&id);
                if let Err(error) = retry_task(state, mineru, active, semaphore, &id).await {
                    state.storage.set_status(
                        &id,
                        JobStatus::Failed,
                        Some(&format!("{error:#}")),
                    )?;
                }
            } else if task.status == JobStatus::WaitingParts {
                let parts = state.storage.list_mineru_parts_for_parent(&id)?;
                // Failed/offline parts leave the parent in WaitingParts. Do not let
                // that indefinitely hold back unrelated documents.
                if parts
                    .iter()
                    .any(|part| matches!(part.status, JobStatus::Failed | JobStatus::WaitingMineru))
                    && parts.iter().all(|part| {
                        matches!(
                            part.status,
                            JobStatus::Completed | JobStatus::Failed | JobStatus::WaitingMineru
                        )
                    })
                {
                    state.conversion_priority.remove(&id);
                    continue;
                }
                for part in parts {
                    if part.status == JobStatus::Queued {
                        schedule_new_mineru_part(state, mineru, semaphore, part).await?;
                    }
                }
            }
        } else {
            state.conversion_priority.mark_started(&id);
        }
        return Ok(());
    }
    // The selected file finished (or failed): continue the ordinary queue.
    process_queued(state, mineru, active, semaphore).await
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
    let mut paths = Vec::new();
    for profile in profiles {
        let Some(control) = state.profile_control(&profile.id) else {
            continue;
        };
        let Ok(write_permit) = control.write_permit().await else {
            continue;
        };
        let extensions = enabled_extensions.clone();
        let discovered = tokio::task::spawn_blocking(move || {
            let _write_permit = write_permit;
            scan_profile_tree(&profile, Path::new(&profile.input_dir), &extensions)
        })
        .await??;
        paths.extend(discovered);
    }
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
        let Some(control) = state.profile_control(&profile.id) else {
            return Ok(());
        };
        let Ok(write_permit) = control.write_permit().await else {
            return Ok(());
        };
        let profile_for_scan = profile.clone();
        let enabled_extensions = settings.enabled_extensions;
        let root = path.clone();
        let paths = tokio::task::spawn_blocking(move || {
            let _write_permit = write_permit;
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

async fn run_if_profile_active<T>(
    control: &ProfileRuntimeControl,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _permit = control.write_permit().await?;
    operation()
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
    if (settings.monitoring_paused && !request.is_force())
        || !is_supported(&path)
        || !is_enabled(&path, &settings.enabled_extensions)
    {
        return;
    }
    let Some(profile) = matching_profile(&settings.profiles, &path) else {
        return;
    };
    let Some(control) = state.profile_control(&profile.id) else {
        return;
    };
    if let Ok(Some(task)) = state.storage.find_by_source(&path.to_string_lossy())
        && state.conversion_priority.contains(&task.id)
        && (request == ScheduleRequest::Normal || !state.conversion_priority.allows(&task.id))
    {
        return;
    }
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
    let queued_task_result = run_if_profile_active(&control, || {
        state.storage.queue_task(
            &profile,
            &path,
            relative,
            metadata.len(),
            modified_ms,
            engine.clone(),
            &output,
            request.is_force(),
        )
    })
    .await;
    let queued_task = match queued_task_result {
        Ok(Some(task)) => task,
        Ok(None) => {
            let pending = active.lock().await.finish(&key);
            dispatch_pending(state, key, pending);
            return;
        }
        Err(error) if is_profile_cancelled(&error) => {
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
    let defer_pdf_permit = is_pdf(&path);
    if state.is_paused() || !state.conversion_priority.allows(&queued_task_id) {
        let pending = active.lock().await.finish(&key);
        dispatch_pending(state, key, pending);
        return;
    }
    let state = state.clone();
    let mineru = mineru.clone();
    let active = active.clone();
    let semaphore = semaphore.clone();
    tauri::async_runtime::spawn(async move {
        let result = tokio::select! {
            _ = control.cancelled() => Err(ProfileCancelled.into()),
            result = async {
            let _operation_permit = control.write_permit().await?;
            let part_semaphore = semaphore.clone();
            let _permit = if defer_pdf_permit {
                None
            } else {
                Some(semaphore.acquire_owned().await?)
            };
            if state.is_paused() || !state.conversion_priority.allows(&queued_task_id) {
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
            process_path(
                &state,
                &mineru,
                &current_profile,
                &path,
                request.is_force(),
                &part_semaphore,
                AppState::read_mineru_token,
                &control,
            )
            .await
            } => result,
        };
        if let Err(error) = result
            && !control.is_cancelled()
            && !is_profile_cancelled(&error)
        {
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

#[allow(clippy::too_many_arguments)]
async fn process_path(
    state: &AppState,
    mineru: &MinerUClient,
    profile: &WatchProfile,
    path: &Path,
    force: bool,
    semaphore: &Arc<Semaphore>,
    read_mineru_token: fn() -> Result<String>,
    control: &ProfileRuntimeControl,
) -> Result<()> {
    let metadata = wait_until_stable(path).await?;
    if !is_markdown(path) && metadata.len() > crate::local_conversion::MAX_INPUT_BYTES {
        anyhow::bail!("原文件超过 512 MiB 安全上限");
    }
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
    let mut engine = default_engine(path).context("不支持的文档格式")?;
    if is_pdf(path) && state.storage.pdf_requires_ocr(path, &hash)? {
        engine = ConversionEngine::Mineru;
    }
    let output = output_path(profile, path)?;
    let previous_output = state
        .storage
        .find_by_source(&path.to_string_lossy())?
        .and_then(|task| task.output_path)
        .map(PathBuf::from)
        .filter(|previous| previous != &output && previous.starts_with(&profile.output_dir));
    let Some(mut task) = state.storage.prepare_task(
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

    let result: Result<bool> = match engine {
        ConversionEngine::Anydoc | ConversionEngine::Copy | ConversionEngine::Anytomd => {
            async {
                // Non-PDF tasks hold the scheduling permit; PDFs hold it only
                // during local conversion, releasing it before cloud fan-out.
                let pdf_permit = if is_pdf(path) {
                    Some(semaphore.clone().acquire_owned().await?)
                } else {
                    None
                };
                if state.is_paused() || !state.conversion_priority.allows(&task.id) {
                    return Ok(false);
                }
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
                    let write_permit = control.write_permit().await?;
                    tokio::task::spawn_blocking(move || {
                        let _write_permit = write_permit;
                        copy_markdown(&profile, &source, &output)
                    })
                    .await??;
                } else {
                    let outcome =
                        tokio::task::spawn_blocking(move || convert_locally(&source)).await??;
                    verify_source_version(path, &hash).await?;
                    let artifact = match outcome {
                        LocalConversion::Converted(artifact) => artifact,
                        LocalConversion::NeedsOcr { pages, page_count } => {
                            if !is_pdf(path) {
                                anyhow::bail!("非 PDF 文档请求了 OCR");
                            }
                            state.storage.mark_pdf_requires_ocr(&task.id, &hash)?;
                            engine = ConversionEngine::Mineru;
                            task = state
                                .storage
                                .get_task(&task.id)?
                                .context("OCR 任务不存在")?;
                            tracing::info!(
                                pages_needing_ocr = pages.len(),
                                page_count,
                                "PDF requires OCR"
                            );
                            drop(pdf_permit);
                            if state.is_paused() || !state.conversion_priority.allows(&task.id) {
                                return Ok(false);
                            }
                            return run_mineru(
                                state,
                                mineru,
                                profile,
                                &task,
                                semaphore,
                                read_mineru_token,
                                control,
                            )
                            .await;
                        }
                    };
                    let profile = profile.clone();
                    let task_for_write = task.clone();
                    let write_permit = control.write_permit().await?;
                    tokio::task::spawn_blocking(move || {
                        let _write_permit = write_permit;
                        write_artifact(&profile, &task_for_write, artifact)
                    })
                    .await??;
                }
                Ok(true)
            }
            .await
        }
        ConversionEngine::Mineru => {
            run_mineru(
                state,
                mineru,
                profile,
                &task,
                semaphore,
                read_mineru_token,
                control,
            )
            .await
        }
    };

    match result {
        Ok(true) => {
            if let Some(previous) = previous_output {
                let profile = profile.clone();
                let write_permit = control.write_permit().await?;
                tokio::task::spawn_blocking(move || {
                    let _write_permit = write_permit;
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
        Ok(false) => {}
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
    semaphore: &Arc<Semaphore>,
    read_mineru_token: fn() -> Result<String>,
    control: &ProfileRuntimeControl,
) -> Result<bool> {
    let source = Path::new(&task.source_path);
    if state.is_paused() || !state.conversion_priority.allows(&task.id) {
        return Ok(false);
    }
    let token = read_mineru_token()?;
    if let Some(expected_hash) = task.source_hash.as_deref() {
        verify_source_version(source, expected_hash).await?;
    }
    let is_pdf = source
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    if is_pdf {
        let work_dir = parent_work_dir(state, &task.id);
        let planning_work_dir = work_dir.join(format!("planning-{}", Uuid::new_v4().simple()));
        let work_dir_for_create = work_dir.clone();
        let write_permit = control.write_permit().await?;
        tokio::task::spawn_blocking(move || {
            let _write_permit = write_permit;
            std::fs::create_dir_all(&work_dir_for_create)?;
            Ok::<(), anyhow::Error>(())
        })
        .await??;
        state.storage.delete_mineru_parts(&task.id)?;
        let source_for_plan = source.to_path_buf();
        let work_for_plan = planning_work_dir.clone();
        let write_permit = control.write_permit().await?;
        let plan_result = tokio::task::spawn_blocking(move || {
            let _write_permit = write_permit;
            plan_pdf(&source_for_plan, &work_for_plan)
        })
        .await;
        let plan = match plan_result {
            Ok(Ok(plan)) => plan,
            Ok(Err(error)) => {
                tokio::fs::remove_dir_all(&planning_work_dir).await.ok();
                return Err(error);
            }
            Err(error) => {
                tokio::fs::remove_dir_all(&planning_work_dir).await.ok();
                return Err(error.into());
            }
        };
        if let PdfPlan::Multipart { page_count, parts } = plan {
            let source_hash = task
                .source_hash
                .as_deref()
                .context("MinerU 父任务缺少源文件哈希")?;
            let records =
                state
                    .storage
                    .replace_mineru_parts(&task.id, source_hash, page_count, &parts)?;
            let physical_inputs = parts
                .iter()
                .zip(records.iter())
                .filter_map(|(plan, record)| {
                    plan.input_path
                        .as_ref()
                        .map(|source| (source.clone(), part_input_path(state, record)))
                })
                .collect::<Vec<_>>();
            let write_permit = control.write_permit().await?;
            tokio::task::spawn_blocking(move || {
                let _write_permit = write_permit;
                for (source, destination) in physical_inputs {
                    if let Some(parent) = destination.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::rename(&source, &destination).with_context(|| {
                        format!(
                            "无法安装隔离的 PDF 分片：{} -> {}",
                            source.display(),
                            destination.display()
                        )
                    })?;
                }
                Ok::<(), anyhow::Error>(())
            })
            .await??;
            tokio::fs::remove_dir_all(&planning_work_dir).await.ok();
            for part in records {
                schedule_new_mineru_part(state, mineru, semaphore, part).await?;
            }
            return Ok(false);
        }
        tokio::fs::remove_dir_all(&planning_work_dir).await.ok();
    }

    let _direct_pdf_permit = if is_pdf {
        Some(semaphore.clone().acquire_owned().await?)
    } else {
        None
    };
    if state.is_paused() || !state.conversion_priority.allows(&task.id) {
        return Ok(false);
    }
    let base_url = state.settings.read().await.mineru_base_url.clone();
    let submission = mineru.submit(source, None, &base_url, &token).await?;
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
    let stage_dir = parent_work_dir(state, &task.id).join("single");
    mineru
        .download_to_stage(&result, &stage_dir, control)
        .await?;
    verify_mineru_source_hash(state, task).await?;
    let profile = profile.clone();
    let task = task.clone();
    let task_id = task.id.clone();
    let write_permit = control.write_permit().await?;
    tokio::task::spawn_blocking(move || {
        let _write_permit = write_permit;
        write_staged_mineru_artifact(&profile, &task, &stage_dir)
    })
    .await??;
    let work_dir = parent_work_dir(state, &task_id);
    let cleanup = tokio::task::spawn_blocking(move || {
        if work_dir.exists() {
            std::fs::remove_dir_all(work_dir)?;
        }
        Ok::<(), anyhow::Error>(())
    })
    .await;
    match cleanup {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(error = %format!("{error:#}"), "failed to clean completed MinerU cache");
        }
        Err(error) => {
            tracing::warn!(error = %error, "completed MinerU cache cleanup task failed");
        }
    }
    Ok(true)
}

fn parent_work_dir(state: &AppState, parent_task_id: &str) -> PathBuf {
    state.storage.mineru_work_root().join(parent_task_id)
}

fn part_input_path(state: &AppState, part: &MinerUPartRecord) -> PathBuf {
    parent_work_dir(state, &part.parent_task_id)
        .join("input")
        .join(format!("{}.pdf", part.id))
}

fn part_stage_dir(state: &AppState, part: &MinerUPartRecord) -> PathBuf {
    parent_work_dir(state, &part.parent_task_id)
        .join("artifacts")
        .join(&part.id)
}

async fn schedule_new_mineru_part(
    state: &AppState,
    mineru: &MinerUClient,
    semaphore: &Arc<Semaphore>,
    part: MinerUPartRecord,
) -> Result<()> {
    if state.is_paused()
        || !state.conversion_priority.allows(&part.parent_task_id)
        || !state.storage.claim_queued_mineru_part(&part.id)?
    {
        return Ok(());
    }
    spawn_mineru_part(state, mineru, semaphore, part, false)?;
    Ok(())
}

async fn schedule_resumed_mineru_part(
    state: &AppState,
    mineru: &MinerUClient,
    semaphore: &Arc<Semaphore>,
    part: MinerUPartRecord,
) -> Result<()> {
    if !state.conversion_priority.allows(&part.parent_task_id) {
        return Ok(());
    }
    state
        .storage
        .set_mineru_part_status(&part.id, JobStatus::Processing, None)?;
    spawn_mineru_part(state, mineru, semaphore, part, true)?;
    Ok(())
}

fn spawn_mineru_part(
    state: &AppState,
    mineru: &MinerUClient,
    semaphore: &Arc<Semaphore>,
    part: MinerUPartRecord,
    resume_existing: bool,
) -> Result<()> {
    let Some(parent) = state.storage.get_task(&part.parent_task_id)? else {
        return Ok(());
    };
    let Some(control) = state.profile_control(&parent.profile_id) else {
        return Ok(());
    };
    let state = state.clone();
    let mineru = mineru.clone();
    let semaphore = semaphore.clone();
    tauri::async_runtime::spawn(async move {
        let result = tokio::select! {
            _ = control.cancelled() => Err(ProfileCancelled.into()),
            result = async {
            let _operation_permit = control.write_permit().await?;
            let _permit = semaphore.acquire_owned().await?;
            if (state.is_paused() || !state.conversion_priority.allows(&part.parent_task_id))
                && !resume_existing
            {
                state.storage.reset_mineru_part_for_retry(&part.id)?;
                return Ok(());
            }
            process_mineru_part(&state, &mineru, &part, resume_existing, &control).await
            } => result,
        };
        if result.is_ok() && !control.is_cancelled() {
            if let Err(error) = try_finalize_mineru_parent(&state, &part.parent_task_id).await {
                tracing::error!(
                    parent_task_id = %part.parent_task_id,
                    error = %format!("{error:#}"),
                    "MinerU parent merge failed"
                );
            }
        } else if let Err(error) = result
            && !control.is_cancelled()
            && !is_profile_cancelled(&error)
        {
            let status = mineru_failure_status(&error);
            let _ =
                state
                    .storage
                    .set_mineru_part_status(&part.id, status, Some(&format!("{error:#}")));
            tracing::error!(
                part_id = %part.id,
                parent_task_id = %part.parent_task_id,
                pages = %format!("{}-{}", part.page_start, part.page_end),
                error = %format!("{error:#}"),
                "MinerU part failed"
            );
            if state
                .storage
                .get_mineru_part(&part.id)
                .ok()
                .flatten()
                .is_none()
            {
                tokio::fs::remove_file(part_input_path(&state, &part))
                    .await
                    .ok();
                tokio::fs::remove_dir_all(part_stage_dir(&state, &part))
                    .await
                    .ok();
                if state
                    .storage
                    .get_task(&part.parent_task_id)
                    .ok()
                    .flatten()
                    .is_none()
                {
                    tokio::fs::remove_dir_all(parent_work_dir(&state, &part.parent_task_id))
                        .await
                        .ok();
                }
            }
        }
    });
    Ok(())
}

async fn process_mineru_part(
    state: &AppState,
    mineru: &MinerUClient,
    part: &MinerUPartRecord,
    resume_existing: bool,
    control: &ProfileRuntimeControl,
) -> Result<()> {
    let parent = state
        .storage
        .get_task(&part.parent_task_id)?
        .context("MinerU 分片缺少父任务")?;
    if parent.source_hash.as_deref() != Some(part.source_hash.as_str()) {
        anyhow::bail!("源文件已变化，旧 MinerU 分片结果已作废");
    }
    let source = PathBuf::from(&parent.source_path);
    if !source.is_file() {
        anyhow::bail!("MinerU 分片源文件不存在：{}", source.display());
    }
    let token = AppState::read_mineru_token()?;
    let base_url = state.settings.read().await.mineru_base_url.clone();
    let result = if resume_existing {
        let batch_id = part
            .mineru_batch_id
            .as_deref()
            .context("MinerU 分片缺少 batch_id")?;
        poll_mineru_part(
            state,
            mineru,
            part,
            batch_id,
            part.mineru_data_id.as_deref(),
            &base_url,
            &token,
        )
        .await?
    } else {
        let (upload_path, page_ranges) = match part.mode {
            MinerUPartMode::PageRanges => (
                source.clone(),
                Some(format!("{}-{}", part.page_start, part.page_end)),
            ),
            MinerUPartMode::SplitPdf => {
                let input_path = part_input_path(state, part);
                if !input_path.is_file() {
                    let source_for_split = source.clone();
                    let destination = input_path.clone();
                    let page_start = u32::try_from(part.page_start).context("分片起始页无效")?;
                    let page_end = u32::try_from(part.page_end).context("分片结束页无效")?;
                    let write_permit = control.write_permit().await?;
                    tokio::task::spawn_blocking(move || {
                        let _write_permit = write_permit;
                        recreate_physical_part(
                            &source_for_split,
                            page_start,
                            page_end,
                            &destination,
                        )
                    })
                    .await??;
                }
                (input_path, None)
            }
        };
        let submission = mineru
            .submit(&upload_path, page_ranges.as_deref(), &base_url, &token)
            .await?;
        state.storage.set_mineru_part_submission(
            &part.id,
            &submission.batch_id,
            &submission.data_id,
        )?;
        if let Err(error) = mineru.upload(&upload_path, &submission.upload_url).await {
            state.storage.clear_mineru_part_submission(&part.id)?;
            return Err(error);
        }
        if part.mode == MinerUPartMode::SplitPdf {
            tokio::fs::remove_file(&upload_path).await.ok();
        }
        state
            .storage
            .set_mineru_part_status(&part.id, JobStatus::Processing, None)?;
        poll_mineru_part(
            state,
            mineru,
            part,
            &submission.batch_id,
            Some(&submission.data_id),
            &base_url,
            &token,
        )
        .await?
    };

    state
        .storage
        .set_mineru_part_status(&part.id, JobStatus::Downloading, None)?;
    let stage_dir = part_stage_dir(state, part);
    mineru
        .download_to_stage(&result, &stage_dir, control)
        .await?;
    let current_parent = state.storage.get_task(&part.parent_task_id)?;
    let current_part = state.storage.get_mineru_part(&part.id)?;
    if current_parent
        .as_ref()
        .and_then(|parent| parent.source_hash.as_deref())
        != Some(part.source_hash.as_str())
        || current_part
            .as_ref()
            .is_none_or(|current| current.source_hash != part.source_hash)
    {
        tokio::fs::remove_dir_all(&stage_dir).await.ok();
        if current_parent.is_none() {
            tokio::fs::remove_dir_all(parent_work_dir(state, &part.parent_task_id))
                .await
                .ok();
        }
        return Ok(());
    }
    state.storage.complete_mineru_part(&part.id)?;
    Ok(())
}

async fn poll_mineru_part(
    state: &AppState,
    mineru: &MinerUClient,
    part: &MinerUPartRecord,
    batch_id: &str,
    data_id: Option<&str>,
    base_url: &str,
    token: &str,
) -> Result<crate::mineru::ExtractResult> {
    let progress_storage = state.storage.clone();
    let progress_part_id = part.id.clone();
    mineru
        .poll(batch_id, data_id, base_url, token, move |item| {
            let progress = item.extract_progress.as_ref();
            progress_storage.set_mineru_part_progress(
                &progress_part_id,
                item.state.as_deref(),
                progress.and_then(|value| value.extracted_pages),
                progress.and_then(|value| value.total_pages),
                progress.and_then(|value| value.start_time.as_deref()),
            )
        })
        .await
}

async fn try_finalize_mineru_parent(state: &AppState, parent_task_id: &str) -> Result<()> {
    let Some(parent) = state.storage.get_task(parent_task_id)? else {
        return Ok(());
    };
    let Some(control) = state.profile_control(&parent.profile_id) else {
        return Ok(());
    };
    let _operation_permit = control.write_permit().await?;
    if !state.storage.claim_parent_for_merge(parent_task_id)? {
        return Ok(());
    }
    let result = async {
        let parent = state
            .storage
            .get_task(parent_task_id)?
            .context("MinerU 父任务不存在")?;
        let parts = state
            .storage
            .list_mineru_parts_for_parent(parent_task_id)?;
        if parts
            .iter()
            .any(|part| parent.source_hash.as_deref() != Some(part.source_hash.as_str()))
        {
            anyhow::bail!("源文件已变化，不能合并旧 MinerU 分片");
        }
        verify_mineru_source_hash(state, &parent).await?;
        let settings = state.settings.read().await.clone();
        let profile = settings
            .profiles
            .into_iter()
            .find(|profile| profile.id == parent.profile_id)
            .context("MinerU 父任务所属目录不存在")?;
        let staged = parts
            .iter()
            .map(|part| StagedMinerUPart {
                index: part.part_index,
                page_start: part.page_start,
                page_end: part.page_end,
                stage_dir: part_stage_dir(state, part),
            })
            .collect::<Vec<_>>();
        let profile_for_write = profile.clone();
        let parent_for_write = parent.clone();
        let write_permit = control.write_permit().await?;
        tokio::task::spawn_blocking(move || {
            let _write_permit = write_permit;
            write_multipart_artifact(&profile_for_write, &parent_for_write, &staged)
        })
        .await??;
        state
            .storage
            .set_status(parent_task_id, JobStatus::Completed, None)?;
        if profile.tagging.enabled
            && !profile.tagging.labels.is_empty()
            && let Some(output) = parent.output_path.as_deref()
        {
            let _ = state.send_tag_runtime(TagRuntimeMessage::Path {
                path: PathBuf::from(output),
                force: true,
            });
        }
        let work_dir = parent_work_dir(state, parent_task_id);
        let cleanup = tokio::task::spawn_blocking(move || {
            if work_dir.exists() {
                std::fs::remove_dir_all(work_dir)?;
            }
            Ok::<(), anyhow::Error>(())
        })
        .await;
        match cleanup {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(error = %format!("{error:#}"), "failed to clean completed multipart MinerU cache");
            }
            Err(error) => {
                tracing::warn!(error = %error, "completed multipart MinerU cache cleanup task failed");
            }
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(error) = result {
        if control.is_cancelled() || is_profile_cancelled(&error) {
            return Ok(());
        }
        state.storage.set_status(
            parent_task_id,
            JobStatus::Failed,
            Some(&format!("MinerU 分片合并失败：{error:#}")),
        )?;
        return Err(error);
    }
    Ok(())
}

async fn resume_interrupted(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
) -> Result<()> {
    state.storage.reset_interrupted_parent_merges()?;
    let parts = state.storage.list_mineru_parts_with_statuses(&[
        JobStatus::Queued,
        JobStatus::Uploading,
        JobStatus::Processing,
        JobStatus::Downloading,
        JobStatus::Completed,
    ])?;
    let mut parents_to_finalize = HashSet::new();
    for part in parts {
        parents_to_finalize.insert(part.parent_task_id.clone());
        match part.status {
            JobStatus::Completed if part.artifact_ready => {}
            JobStatus::Processing | JobStatus::Downloading if part.mineru_batch_id.is_some() => {
                schedule_resumed_mineru_part(state, mineru, semaphore, part).await?;
            }
            JobStatus::Uploading => {
                state.storage.reset_mineru_part_for_retry(&part.id)?;
                if !state.is_paused() {
                    let part = state
                        .storage
                        .get_mineru_part(&part.id)?
                        .context("重启恢复的 MinerU 分片不存在")?;
                    schedule_new_mineru_part(state, mineru, semaphore, part).await?;
                }
            }
            JobStatus::Queued if !state.is_paused() => {
                schedule_new_mineru_part(state, mineru, semaphore, part).await?;
            }
            _ => {}
        }
    }
    for parent_id in parents_to_finalize {
        try_finalize_mineru_parent(state, &parent_id).await.ok();
    }

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
    if !state.conversion_priority.allows(&task.id)
        || !path.is_file()
        || !active.lock().await.try_start(&path, request)
    {
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
    let Some(control) = state.profile_control(&profile.id) else {
        let pending = active.lock().await.finish(&path);
        dispatch_pending(state, path, pending);
        return;
    };
    let state = state.clone();
    let mineru = mineru.clone();
    let active = active.clone();
    let semaphore = semaphore.clone();
    tauri::async_runtime::spawn(async move {
        let result = tokio::select! {
            _ = control.cancelled() => Err(ProfileCancelled.into()),
            result = async {
            let _operation_permit = control.write_permit().await?;
            let _permit = semaphore.acquire_owned().await?;
            if !state.conversion_priority.allows(&task.id) { return Ok(()); }
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
            let stage_dir = parent_work_dir(&state, &task.id).join("single");
            mineru.download_to_stage(&result, &stage_dir, &control).await?;
            verify_mineru_source_hash(&state, &task).await?;
            let profile_for_write = profile.clone();
            let task_for_write = task.clone();
            let write_permit = control.write_permit().await?;
            tokio::task::spawn_blocking(move || {
                let _write_permit = write_permit;
                write_staged_mineru_artifact(&profile_for_write, &task_for_write, &stage_dir)
            })
            .await??;
            state
                .storage
                .set_status(&task.id, JobStatus::Completed, None)?;
            if profile.tagging.enabled
                && !profile.tagging.labels.is_empty()
                && let Some(output) = task.output_path.as_deref()
            {
                let _ = state.send_tag_runtime(TagRuntimeMessage::Path {
                    path: PathBuf::from(output),
                    force: true,
                });
            }
            let work_dir = parent_work_dir(&state, &task.id);
            let cleanup = tokio::task::spawn_blocking(move || {
                if work_dir.exists() {
                    std::fs::remove_dir_all(work_dir)?;
                }
                Ok::<(), anyhow::Error>(())
            })
            .await;
            match cleanup {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::warn!(error = %format!("{error:#}"), "failed to clean resumed MinerU cache");
                }
                Err(error) => {
                    tracing::warn!(error = %error, "resumed MinerU cache cleanup task failed");
                }
            }
            Ok::<(), anyhow::Error>(())
            } => result,
        };
        if let Err(error) = result
            && !control.is_cancelled()
            && !is_profile_cancelled(&error)
        {
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

async fn verify_mineru_source_hash(state: &AppState, task: &TaskRecord) -> Result<()> {
    let expected = task
        .source_hash
        .as_deref()
        .context("MinerU 任务缺少源文件哈希")?;
    let source = PathBuf::from(&task.source_path);
    let source_for_hash = source.clone();
    let actual = tokio::task::spawn_blocking(move || sha256_file(&source_for_hash)).await??;
    if actual != expected {
        let _ = state.send_runtime(RuntimeMessage::Path {
            path: source,
            force: true,
        });
        anyhow::bail!("源文件已变化，当前 MinerU 结果已作废");
    }
    Ok(())
}

async fn retry_task(
    state: &AppState,
    mineru: &MinerUClient,
    active: &SharedActivePaths,
    semaphore: &Arc<Semaphore>,
    task_id: &str,
) -> Result<()> {
    if let Some(part) = state.storage.get_mineru_part(task_id)? {
        let parent = state
            .storage
            .get_task(&part.parent_task_id)?
            .context("MinerU 分片父任务不存在")?;
        let source = PathBuf::from(&parent.source_path);
        if !source.is_file() {
            anyhow::bail!("MinerU 分片源文件不存在：{}", source.display());
        }
        let source_for_hash = source.clone();
        let current_hash =
            tokio::task::spawn_blocking(move || sha256_file(&source_for_hash)).await??;
        if parent.source_hash.as_deref() != Some(current_hash.as_str())
            || part.source_hash != current_hash
        {
            schedule_path(
                state,
                mineru,
                active,
                semaphore,
                source,
                ScheduleRequest::Force,
            )
            .await;
            return Ok(());
        }
        let stage_dir = part_stage_dir(state, &part);
        tokio::task::spawn_blocking(move || {
            if stage_dir.exists() {
                std::fs::remove_dir_all(stage_dir)?;
            }
            Ok::<(), anyhow::Error>(())
        })
        .await??;
        state.storage.reset_mineru_part_for_retry(&part.id)?;
        state
            .storage
            .set_parent_waiting_parts(&part.parent_task_id)?;
        if !state.is_paused() {
            let part = state
                .storage
                .get_mineru_part(&part.id)?
                .context("待重试的 MinerU 分片不存在")?;
            schedule_new_mineru_part(state, mineru, semaphore, part).await?;
        }
        return Ok(());
    }

    let task = state.storage.get_task(task_id)?.context("任务不存在")?;
    let parts = state.storage.list_mineru_parts_for_parent(&task.id)?;
    if !parts.is_empty() {
        let source = PathBuf::from(&task.source_path);
        if !source.is_file() {
            anyhow::bail!("MinerU 父任务源文件不存在：{}", source.display());
        }
        let source_for_hash = source.clone();
        let current_hash =
            tokio::task::spawn_blocking(move || sha256_file(&source_for_hash)).await??;
        if task.source_hash.as_deref() != Some(current_hash.as_str())
            || parts.iter().any(|part| part.source_hash != current_hash)
        {
            schedule_path(
                state,
                mineru,
                active,
                semaphore,
                source,
                ScheduleRequest::Force,
            )
            .await;
            return Ok(());
        }
        state.storage.set_parent_waiting_parts(&task.id)?;
        for part in parts {
            match part.status {
                JobStatus::Failed => {
                    state.storage.reset_mineru_part_for_retry(&part.id)?;
                    if !state.is_paused() {
                        let part = state
                            .storage
                            .get_mineru_part(&part.id)?
                            .context("待重试的 MinerU 分片不存在")?;
                        schedule_new_mineru_part(state, mineru, semaphore, part).await?;
                    }
                }
                JobStatus::Queued if !state.is_paused() => {
                    schedule_new_mineru_part(state, mineru, semaphore, part).await?;
                }
                JobStatus::WaitingMineru if !state.is_paused() => {
                    if part.mineru_batch_id.is_some() {
                        schedule_resumed_mineru_part(state, mineru, semaphore, part).await?;
                    } else {
                        state.storage.reset_mineru_part_for_retry(&part.id)?;
                        let part = state
                            .storage
                            .get_mineru_part(&part.id)?
                            .context("待重试的 MinerU 分片不存在")?;
                        schedule_new_mineru_part(state, mineru, semaphore, part).await?;
                    }
                }
                _ => {}
            }
        }
        try_finalize_mineru_parent(state, &task.id).await?;
        return Ok(());
    }
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
    for part in state
        .storage
        .list_mineru_parts_with_statuses(&[JobStatus::Queued])?
    {
        schedule_new_mineru_part(state, mineru, semaphore, part).await?;
    }
    let tasks = state.storage.list_tasks_with_statuses(&[
        JobStatus::WaitingStable,
        JobStatus::Queued,
        JobStatus::Converting,
        JobStatus::Uploading,
    ])?;
    for task in tasks {
        let source = PathBuf::from(task.source_path);
        if source.is_file()
            && state.conversion_priority.allows(&task.id)
            && !active.lock().await.running.contains(&source)
        {
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
    for part in state
        .storage
        .list_mineru_parts_with_statuses(&[JobStatus::WaitingMineru])?
    {
        if part.mineru_batch_id.is_some() {
            schedule_resumed_mineru_part(state, mineru, semaphore, part).await?;
        } else {
            state.storage.reset_mineru_part_for_retry(&part.id)?;
            let part = state
                .storage
                .get_mineru_part(&part.id)?
                .context("待重试的 MinerU 分片不存在")?;
            schedule_new_mineru_part(state, mineru, semaphore, part).await?;
        }
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

async fn reconcile_missing_sources(state: &AppState, active: &SharedActivePaths) -> Result<()> {
    let settings = state.settings.read().await.clone();
    for profile in settings
        .profiles
        .into_iter()
        .filter(|profile| profile.enabled)
    {
        let Some(control) = state.profile_control(&profile.id) else {
            continue;
        };
        let Ok(write_permit) = control.write_permit().await else {
            continue;
        };
        let input_root = Path::new(&profile.input_dir);
        let root_available = std::fs::read_dir(input_root)
            .and_then(|mut entries| entries.next().transpose().map(|_| ()))
            .is_ok();
        if !root_available {
            tracing::warn!(
                profile_id = %profile.id,
                input_root = %input_root.display(),
                "source root is unavailable; skipping deletion reconciliation"
            );
            continue;
        }
        for task in state.storage.list_profile_tasks(&profile.id)? {
            let source = Path::new(&task.source_path);
            match std::fs::symlink_metadata(source) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::warn!(
                        task_id = %task.id,
                        source = %source.display(),
                        error = %error,
                        "source status is unknown; skipping deletion reconciliation"
                    );
                    continue;
                }
            }
            if active.lock().await.running.contains(source) {
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
            let work_dir = parent_work_dir(state, &task.id);
            if let Err(error) = tokio::task::spawn_blocking(move || {
                if work_dir.exists()
                    && let Err(error) = std::fs::remove_dir_all(&work_dir)
                {
                    tracing::warn!(path = %work_dir.display(), error = %error, "failed to clean deleted source MinerU cache");
                }
            })
            .await
            {
                tracing::warn!(error = %error, "deleted source MinerU cache cleanup task failed");
            }
        }
        tokio::task::spawn_blocking(move || {
            let _write_permit = write_permit;
            prune_empty_output_directories(&profile)
        })
        .await??;
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

async fn verify_source_version(path: &Path, expected_hash: &str) -> Result<()> {
    let source = path.to_path_buf();
    let actual = tokio::task::spawn_blocking(move || sha256_file(&source)).await??;
    if actual != expected_hash {
        anyhow::bail!("源文件已变化，当前转换结果已作废");
    }
    Ok(())
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
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(encoded)
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

    fn no_cloud_token() -> Result<String> {
        panic!("local conversion must not read cloud credentials");
    }

    fn missing_cloud_token() -> Result<String> {
        anyhow::bail!("未配置 MinerU Token");
    }

    #[tokio::test]
    async fn local_documents_write_assets_and_pdf_never_reads_cloud_credentials() {
        use crate::local_conversion::tests::{write_docx, write_pdf};
        let (_root, state, profile, _active, mineru, semaphore) = priority_fixture().await;
        state.set_paused_flag(false);
        let source = Path::new(&profile.input_dir).join("document.docx");
        write_docx(&source);
        process_path(
            &state,
            &mineru,
            &profile,
            &source,
            false,
            &semaphore,
            no_cloud_token,
            &state.profile_control(&profile.id).unwrap(),
        )
        .await
        .unwrap();
        let task = state
            .storage
            .find_by_source(&source.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(task.status, JobStatus::Completed);
        assert_eq!(task.engine, ConversionEngine::Anydoc);
        let output = output_path(&profile, &source).unwrap();
        let markdown = std::fs::read_to_string(&output).unwrap();
        assert!(markdown.contains("converter: anydoc"));
        assert!(
            markdown.contains("document.assets/asset-0000.png"),
            "{markdown}"
        );
        let assets = crate::converter::asset_path_for_output(&output).unwrap();
        assert_eq!(
            std::fs::read(assets.join("asset-0000.png")).unwrap(),
            b"synthetic image payload preserved verbatim"
        );

        let pdf = Path::new(&profile.input_dir).join("text.pdf");
        write_pdf(&pdf, &[false]);
        process_path(
            &state,
            &mineru,
            &profile,
            &pdf,
            false,
            &semaphore,
            no_cloud_token,
            &state.profile_control(&profile.id).unwrap(),
        )
        .await
        .unwrap();
        let task = state
            .storage
            .find_by_source(&pdf.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(task.status, JobStatus::Completed);
        assert_eq!(task.engine, ConversionEngine::Anydoc);
        std::fs::write(&pdf, b"%PDF-1.5 invalid document").unwrap();
        process_path(
            &state,
            &mineru,
            &profile,
            &pdf,
            true,
            &semaphore,
            no_cloud_token,
            &state.profile_control(&profile.id).unwrap(),
        )
        .await
        .unwrap();
        let task = state.storage.get_task(&task.id).unwrap().unwrap();
        assert_eq!(task.status, JobStatus::Failed);
        assert_eq!(task.engine, ConversionEngine::Anydoc);
        assert!(task.mineru_batch_id.is_none());
    }

    #[tokio::test]
    async fn pdf_ocr_decision_survives_restart_retry_and_clears_on_source_change() {
        use crate::local_conversion::tests::write_pdf;
        let (root, state, profile, _active, mineru, semaphore) = priority_fixture().await;
        state.set_paused_flag(false);
        let source = Path::new(&profile.input_dir).join("scan.pdf");
        write_pdf(&source, &[false, true]);
        process_path(
            &state,
            &mineru,
            &profile,
            &source,
            false,
            &semaphore,
            missing_cloud_token,
            &state.profile_control(&profile.id).unwrap(),
        )
        .await
        .unwrap();
        let task = state
            .storage
            .find_by_source(&source.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(task.status, JobStatus::WaitingMineru);
        assert_eq!(task.engine, ConversionEngine::Mineru);
        let hash = task.source_hash.as_ref().unwrap();
        assert!(state.storage.pdf_requires_ocr(&source, hash).unwrap());
        assert!(!output_path(&profile, &source).unwrap().exists());
        assert_eq!(semaphore.available_permits(), 2);
        let reopened = crate::storage::Storage::new(root.path().join("data")).unwrap();
        assert!(reopened.pdf_requires_ocr(&source, hash).unwrap());
        assert!(
            reopened
                .mark_pdf_requires_ocr(&task.id, "stale-hash")
                .is_err()
        );

        // Queueing before hashing must preserve the previous decision.
        let metadata = std::fs::metadata(&source).unwrap();
        reopened
            .queue_task(
                &profile,
                &source,
                Path::new("scan.pdf"),
                metadata.len(),
                0,
                ConversionEngine::Anydoc,
                &output_path(&profile, &source).unwrap(),
                true,
            )
            .unwrap();
        assert!(reopened.pdf_requires_ocr(&source, hash).unwrap());
        process_path(
            &state,
            &mineru,
            &profile,
            &source,
            true,
            &semaphore,
            missing_cloud_token,
            &state.profile_control(&profile.id).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            state.storage.get_task(&task.id).unwrap().unwrap().status,
            JobStatus::WaitingMineru
        );

        write_pdf(&source, &[false]);
        process_path(
            &state,
            &mineru,
            &profile,
            &source,
            true,
            &semaphore,
            no_cloud_token,
            &state.profile_control(&profile.id).unwrap(),
        )
        .await
        .unwrap();
        let changed = state.storage.get_task(&task.id).unwrap().unwrap();
        assert_eq!(changed.status, JobStatus::Completed);
        assert_eq!(changed.engine, ConversionEngine::Anydoc);
        assert!(!reopened.pdf_requires_ocr(&source, hash).unwrap());
        assert!(
            !reopened
                .pdf_requires_ocr(&source, changed.source_hash.as_ref().unwrap())
                .unwrap()
        );
    }

    #[tokio::test]
    async fn mixed_pdf_fallback_uploads_whole_document_with_one_worker_permit() {
        use crate::local_conversion::tests::write_pdf;
        use std::io::Write as _;
        use std::net::TcpListener;
        let (_root, state, profile, _active, mineru, _semaphore) = priority_fixture().await;
        state.set_paused_flag(false);
        let semaphore = Arc::new(Semaphore::new(1));
        let source = Path::new(&profile.input_dir).join("mixed.pdf");
        write_pdf(&source, &[false, true]);
        let expected_pdf = std::fs::read(&source).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        state.settings.write().await.mineru_base_url = base.clone();
        listener.set_nonblocking(true).unwrap();
        let server = tokio::task::spawn_blocking(move || {
            let mut data_id = String::new();
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            for step in 0..4 {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "mock request timed out at step {step}"
                            );
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let body_start = loop {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buf[..n]);
                    if let Some(pos) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                        break pos + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&request[..body_start]).to_lowercase();
                let content_length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map(|value| value.trim().parse().unwrap())
                    .unwrap_or(0);
                while request.len() < body_start + content_length {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buf[..n]);
                }
                let body = &request[body_start..];
                let response = match step {
                    0 => {
                        assert!(headers.starts_with("post /file-urls/batch "));
                        let json: serde_json::Value = serde_json::from_slice(body).unwrap();
                        assert_eq!(json["files"][0]["is_ocr"], true);
                        assert!(json["files"][0].get("page_ranges").is_none());
                        data_id = json["files"][0]["data_id"].as_str().unwrap().to_string();
                        serde_json::json!({"code":0,"data":{"batch_id":"mock-batch","file_urls":[format!("{base}/upload")]}}).to_string()
                    }
                    1 => {
                        assert!(headers.starts_with("put /upload "));
                        assert_eq!(body, expected_pdf);
                        "{}".into()
                    }
                    2 => {
                        assert!(headers.starts_with("get /extract-results/batch/mock-batch "));
                        serde_json::json!({"code":0,"data":{"extract_result":[{"data_id":data_id,"state":"done","full_markdown_url":format!("{base}/result")}]}}).to_string()
                    }
                    _ => {
                        assert!(headers.starts_with("get /result "));
                        "# OCR fixture\nBoth pages converted.\n".into()
                    }
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            }
        });
        tokio::time::timeout(
            Duration::from_secs(15),
            process_path(
                &state,
                &mineru,
                &profile,
                &source,
                false,
                &semaphore,
                || Ok("synthetic-token".into()),
                &state.profile_control(&profile.id).unwrap(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        server.await.unwrap();
        let task = state
            .storage
            .find_by_source(&source.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(task.status, JobStatus::Completed, "{:?}", task.error);
        assert_eq!(task.engine, ConversionEngine::Mineru);
        let markdown = std::fs::read_to_string(output_path(&profile, &source).unwrap()).unwrap();
        assert!(markdown.contains("converter: mineru"));
        assert!(markdown.contains("Both pages converted"));
        assert_eq!(semaphore.available_permits(), 1);
    }

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

    fn completed_task(
        state: &AppState,
        profile: &WatchProfile,
        source: &Path,
    ) -> (PathBuf, PathBuf, TaskRecord) {
        let source = dunce::canonicalize(source).unwrap();
        let metadata = std::fs::metadata(&source).unwrap();
        let output = output_path(profile, &source).unwrap();
        std::fs::write(&output, b"generated").unwrap();
        let task = state
            .storage
            .queue_task(
                profile,
                &source,
                source.strip_prefix(&profile.input_dir).unwrap(),
                metadata.len(),
                1,
                ConversionEngine::Anytomd,
                &output,
                false,
            )
            .unwrap()
            .unwrap();
        state
            .storage
            .set_status(&task.id, JobStatus::Completed, None)
            .unwrap();
        (source, output, task)
    }

    #[tokio::test]
    async fn stale_scanner_cannot_queue_after_profile_cancellation() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        std::fs::create_dir_all(&input).unwrap();
        let profile = temporary_profile(&input, &output);
        let source = input.join("notes.md");
        let generated = output.join("notes.md");
        std::fs::write(&source, b"# source").unwrap();
        let state = AppState::new(temporary.path().join("data")).unwrap();
        state.ensure_profile_controls([profile.id.as_str()]);
        let stale_control = state.profile_control(&profile.id).unwrap();

        state.begin_profile_removal(&profile.id);
        let result = run_if_profile_active(&stale_control, || {
            state.storage.queue_task(
                &profile,
                &source,
                Path::new("notes.md"),
                8,
                1,
                ConversionEngine::Anytomd,
                &generated,
                false,
            )
        })
        .await;

        assert!(
            result
                .err()
                .is_some_and(|error| is_profile_cancelled(&error))
        );
        assert_eq!(state.storage.task_count().unwrap(), 0);
    }

    async fn priority_fixture() -> (
        tempfile::TempDir,
        AppState,
        WatchProfile,
        SharedActivePaths,
        MinerUClient,
        Arc<Semaphore>,
    ) {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("input");
        std::fs::create_dir_all(&input).unwrap();
        let profile = temporary_profile(&input, &root.path().join("output"));
        let state = AppState::new(root.path().join("data")).unwrap();
        {
            let mut settings = state.settings.write().await;
            settings.profiles = vec![profile.clone()];
            state.ensure_profile_controls([profile.id.as_str()]);
            settings.paused = true;
            settings.monitoring_paused = true;
            settings.classification_paused = true;
        }
        state.set_paused_flag(true);
        state.set_monitoring_paused_flag(true);
        state.set_classification_paused_flag(true);
        (
            root,
            state,
            profile,
            Arc::new(Mutex::new(ActivePaths::default())),
            MinerUClient::new().unwrap(),
            Arc::new(Semaphore::new(2)),
        )
    }

    async fn wait_for_completion(state: &AppState, source: &Path) {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if state
                    .storage
                    .find_by_source(&source.to_string_lossy())
                    .unwrap()
                    .is_some_and(|task| task.status == JobStatus::Completed)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn selected_file_runs_first_then_resumes_queue_without_resuming_classification() {
        let (_root, state, profile, active, mineru, semaphore) = priority_fixture().await;
        let other = Path::new(&profile.input_dir).join("other.md");
        let selected = Path::new(&profile.input_dir).join("selected.md");
        std::fs::write(&other, "# other").unwrap();
        std::fs::write(&selected, "# selected").unwrap();
        schedule_path(
            &state,
            &mineru,
            &active,
            &semaphore,
            other.clone(),
            ScheduleRequest::Force,
        )
        .await;
        queue_priority_file(&state, &active, &profile.id, "selected.md")
            .await
            .unwrap();
        // A second click is coalesced, including while the file waits for a worker.
        queue_priority_file(&state, &active, &profile.id, "selected.md")
            .await
            .unwrap();
        assert!(!state.is_paused());
        assert!(state.is_classification_paused());
        assert!(state.is_monitoring_paused());
        assert!(!state.storage.load_settings().unwrap().paused);
        process_queued(&state, &mineru, &active, &semaphore)
            .await
            .unwrap();
        pump_priority_files(&state, &mineru, &active, &semaphore)
            .await
            .unwrap();
        wait_for_completion(&state, &selected).await;
        assert_eq!(
            state
                .storage
                .find_by_source(&other.to_string_lossy())
                .unwrap()
                .unwrap()
                .status,
            JobStatus::Queued
        );
        // Allow the worker to release its active path before pumping completion.
        tokio::time::timeout(Duration::from_secs(3), async {
            while state.conversion_priority.front().is_some() {
                pump_priority_files(&state, &mineru, &active, &semaphore)
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        wait_for_completion(&state, &other).await;
        assert_eq!(
            std::fs::read_to_string(output_path(&profile, &selected).unwrap()).unwrap(),
            "# selected"
        );
    }

    #[tokio::test]
    async fn missing_priority_file_does_not_block_later_tasks() {
        let (_root, state, profile, active, mineru, semaphore) = priority_fixture().await;
        let missing = Path::new(&profile.input_dir).join("missing.md");
        let next = Path::new(&profile.input_dir).join("next.md");
        std::fs::write(&missing, "# missing").unwrap();
        std::fs::write(&next, "# next").unwrap();
        queue_priority_file(&state, &active, &profile.id, "missing.md")
            .await
            .unwrap();
        queue_priority_file(&state, &active, &profile.id, "next.md")
            .await
            .unwrap();
        std::fs::remove_file(&missing).unwrap();
        pump_priority_files(&state, &mineru, &active, &semaphore)
            .await
            .unwrap();
        wait_for_completion(&state, &next).await;
        assert_eq!(
            state
                .storage
                .find_by_source(&missing.to_string_lossy())
                .unwrap()
                .unwrap()
                .status,
            JobStatus::Failed
        );
    }

    #[tokio::test]
    async fn stalled_pdf_parts_release_priority_and_completed_pdf_reconversion_discards_old_parts()
    {
        let (_root, state, profile, active, mineru, semaphore) = priority_fixture().await;
        let source = Path::new(&profile.input_dir).join("large.pdf");
        std::fs::write(&source, "test pdf record").unwrap();
        queue_priority_file(&state, &active, &profile.id, "large.pdf")
            .await
            .unwrap();
        let task = state
            .storage
            .find_by_source(&source.to_string_lossy())
            .unwrap()
            .unwrap();
        let plans = vec![crate::pdf_split::PdfPartPlan {
            index: 1,
            count: 1,
            page_start: 1,
            page_end: 200,
            mode: MinerUPartMode::PageRanges,
            input_path: None,
        }];
        let parts = state
            .storage
            .replace_mineru_parts(&task.id, "hash", 200, &plans)
            .unwrap();
        state.conversion_priority.mark_started(&task.id);
        state
            .storage
            .set_mineru_part_status(&parts[0].id, JobStatus::WaitingMineru, Some("offline"))
            .unwrap();
        pump_priority_files(&state, &mineru, &active, &semaphore)
            .await
            .unwrap();
        assert!(state.conversion_priority.front().is_none());
        assert_eq!(
            state.storage.get_task(&task.id).unwrap().unwrap().status,
            JobStatus::WaitingParts
        );
        // No provider is contacted; this checks that an explicit reconversion
        // queues a fresh conversion instead of reusing old successful fragments.
        state.storage.complete_mineru_part(&parts[0].id).unwrap();
        state
            .storage
            .set_status(&task.id, JobStatus::Completed, None)
            .unwrap();
        queue_priority_file(&state, &active, &profile.id, "large.pdf")
            .await
            .unwrap();
        assert!(
            state
                .storage
                .list_mineru_parts_for_parent(&task.id)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            state.storage.get_task(&task.id).unwrap().unwrap().status,
            JobStatus::Queued
        );
    }

    #[tokio::test]
    async fn changing_profile_root_releases_old_priority_file() {
        let (root, state, profile, active, mineru, semaphore) = priority_fixture().await;
        let old_source = Path::new(&profile.input_dir).join("file.md");
        std::fs::write(&old_source, "old").unwrap();
        queue_priority_file(&state, &active, &profile.id, "file.md")
            .await
            .unwrap();
        let replacement = root.path().join("replacement");
        std::fs::create_dir_all(&replacement).unwrap();
        std::fs::write(replacement.join("file.md"), "different file").unwrap();
        state.settings.write().await.profiles[0].input_dir = dunce::canonicalize(replacement)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        pump_priority_files(&state, &mineru, &active, &semaphore)
            .await
            .unwrap();
        assert!(state.conversion_priority.front().is_none());
        assert_eq!(
            state
                .storage
                .find_by_source(&old_source.to_string_lossy())
                .unwrap()
                .unwrap()
                .status,
            JobStatus::Failed
        );
    }

    #[tokio::test]
    async fn priority_rejects_traversal_and_disabled_profiles_without_resuming() {
        let (_root, state, profile, active, _mineru, _semaphore) = priority_fixture().await;
        assert!(
            queue_priority_file(&state, &active, &profile.id, "../outside.md")
                .await
                .is_err()
        );
        std::fs::write(Path::new(&profile.input_dir).join("file.md"), "# file").unwrap();
        state.settings.write().await.profiles[0].enabled = false;
        assert!(
            queue_priority_file(&state, &active, &profile.id, "file.md")
                .await
                .is_err()
        );
        assert!(state.is_paused());
        assert!(state.conversion_priority.front().is_none());
    }

    #[tokio::test]
    async fn deleted_profile_cannot_be_requeued_by_a_stale_file_action() {
        let (_root, state, profile, active, _mineru, _semaphore) = priority_fixture().await;
        std::fs::write(Path::new(&profile.input_dir).join("file.md"), "# file").unwrap();
        state.begin_profile_removal(&profile.id);
        let error = queue_priority_file(&state, &active, &profile.id, "file.md")
            .await
            .unwrap_err();
        assert!(is_profile_cancelled(&error));
        assert_eq!(state.storage.task_count().unwrap(), 0);
        assert!(state.conversion_priority.front().is_none());
        assert!(state.is_paused());
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
    async fn reconciliation_keeps_outputs_when_source_root_is_unavailable() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output_root = temporary.path().join("output");
        std::fs::create_dir_all(&input).unwrap();
        let source = input.join("notes.md");
        std::fs::write(&source, b"# offline root").unwrap();
        let profile = temporary_profile(&input, &output_root);
        let state = AppState::new(temporary.path().join("data")).unwrap();
        state.settings.write().await.profiles = vec![profile.clone()];
        state.ensure_profile_controls([profile.id.as_str()]);
        let (_, output, task) = completed_task(&state, &profile, &source);

        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(input).unwrap();
        reconcile_missing_sources(&state, &Arc::new(Mutex::new(ActivePaths::default())))
            .await
            .unwrap();

        assert!(output.is_file());
        assert!(state.storage.get_task(&task.id).unwrap().is_some());
    }

    #[tokio::test]
    async fn reconciliation_skips_disabled_profiles_and_running_paths() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output_root = temporary.path().join("output");
        std::fs::create_dir_all(&input).unwrap();
        let source = input.join("notes.md");
        std::fs::write(&source, b"# active task").unwrap();
        let profile = temporary_profile(&input, &output_root);
        let state = AppState::new(temporary.path().join("data")).unwrap();
        state.settings.write().await.profiles = vec![profile.clone()];
        state.ensure_profile_controls([profile.id.as_str()]);
        let (source, output, task) = completed_task(&state, &profile, &source);
        std::fs::remove_file(&source).unwrap();

        let active = Arc::new(Mutex::new(ActivePaths::default()));
        assert!(
            active
                .lock()
                .await
                .try_start(&source, ScheduleRequest::Normal)
        );
        reconcile_missing_sources(&state, &active).await.unwrap();
        assert!(output.is_file());
        assert!(state.storage.get_task(&task.id).unwrap().is_some());

        active.lock().await.finish(&source);
        state.settings.write().await.profiles[0].enabled = false;
        reconcile_missing_sources(&state, &active).await.unwrap();
        assert!(output.is_file());
        assert!(state.storage.get_task(&task.id).unwrap().is_some());

        state.settings.write().await.profiles[0].enabled = true;
        reconcile_missing_sources(&state, &active).await.unwrap();
        assert!(!output.exists());
        assert!(state.storage.get_task(&task.id).unwrap().is_none());
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
        state.ensure_profile_controls([profile.id.as_str()]);
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
            state.ensure_profile_controls(
                settings.profiles.iter().map(|profile| profile.id.as_str()),
            );
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
        let expected = Path::new(&profile.input_dir)
            .join("客户乙")
            .join("报告.pdf");
        assert_eq!(files, vec![expected]);
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
