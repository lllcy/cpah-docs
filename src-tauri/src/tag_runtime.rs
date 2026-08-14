use crate::knowledge_index::is_profile_index;
use crate::models::{TagJobStatus, WatchProfile};
use crate::state::AppState;
use crate::tagging::{run_tag_agent, schema_hash};
use anyhow::{Context, Result};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub enum TagRuntimeMessage {
    Reload,
    Start,
    Path {
        path: PathBuf,
        force: bool,
    },
    ApplyRules {
        profile_id: String,
        process_existing: bool,
    },
    Retry {
        job_id: String,
    },
    RetryMany {
        job_ids: Vec<String>,
    },
    Pump,
}

#[derive(Debug, Default)]
struct ActiveTagJobs {
    paths: HashSet<PathBuf>,
    force_after: HashSet<PathBuf>,
}

type SharedActive = Arc<Mutex<ActiveTagJobs>>;

pub fn start(state: AppState) -> Result<()> {
    let (sender, receiver) = mpsc::unbounded_channel();
    state.set_tag_runtime_sender(sender.clone());
    let health = state.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run(state, sender, receiver).await {
            let message = format!("Agent 分类后台已停止：{error:#}");
            tracing::error!("{message}");
            health.set_tag_runtime_error(message);
        }
    });
    Ok(())
}

async fn run(
    state: AppState,
    sender: mpsc::UnboundedSender<TagRuntimeMessage>,
    mut receiver: mpsc::UnboundedReceiver<TagRuntimeMessage>,
) -> Result<()> {
    state.storage.requeue_interrupted_tag_jobs()?;
    let event_sender = sender.clone();
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<Event>| match result {
            Ok(event) => {
                for path in event.paths {
                    let _ = event_sender.send(TagRuntimeMessage::Path { path, force: false });
                }
            }
            Err(error) => tracing::error!(error = %error, "tag output watcher error"),
        })?;
    let active = Arc::new(Mutex::new(ActiveTagJobs::default()));
    let mut watched_roots = Vec::new();
    reload_watches(&state, &mut watcher, &mut watched_roots).await?;
    queue_unknown_markdown(&state, &active).await?;
    pump_queue(&state, &active, &sender).await?;

    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.tick().await;
    loop {
        tokio::select! {
            Some(message) = receiver.recv() => {
                match message {
                    TagRuntimeMessage::Reload => {
                        if let Err(error) = reload_watches(&state, &mut watcher, &mut watched_roots).await {
                            tracing::error!(error = %format!("{error:#}"), "tag watcher reload failed");
                        }
                        if !classification_is_blocked(&state)
                            && let Err(error) = queue_unknown_markdown(&state, &active).await
                        {
                            tracing::error!(error = %format!("{error:#}"), "tag output scan failed");
                        }
                    }
                    TagRuntimeMessage::Start => {
                        if let Err(error) = reload_watches(&state, &mut watcher, &mut watched_roots).await {
                            tracing::error!(error = %format!("{error:#}"), "tag watcher start failed");
                        }
                        if !classification_is_blocked(&state) {
                            if let Err(error) = queue_actionable_markdown(&state, &active).await {
                                tracing::error!(error = %format!("{error:#}"), "tag actionable scan failed");
                            }
                            if let Err(error) = queue_unknown_markdown(&state, &active).await {
                                tracing::error!(error = %format!("{error:#}"), "tag output scan failed");
                            }
                        }
                    }
                    TagRuntimeMessage::Path { path, force } => {
                        if let Err(error) = handle_path(&state, &active, path, force).await {
                            tracing::error!(error = %format!("{error:#}"), "tag path scheduling failed");
                        }
                    }
                    TagRuntimeMessage::ApplyRules { profile_id, process_existing } => {
                        if let Err(error) = apply_profile_rules(
                            &state,
                            &active,
                            &profile_id,
                            process_existing,
                        ).await {
                            tracing::error!(error = %format!("{error:#}"), "tag rule application failed");
                        }
                    }
                    TagRuntimeMessage::Retry { job_id } => {
                        if let Err(error) = retry_jobs(&state, &active, &[job_id]).await {
                            tracing::error!(error = %format!("{error:#}"), "tag retry failed");
                        }
                    }
                    TagRuntimeMessage::RetryMany { job_ids } => {
                        if let Err(error) = retry_jobs(&state, &active, &job_ids).await {
                            tracing::error!(error = %format!("{error:#}"), "tag batch retry failed");
                        }
                    }
                    TagRuntimeMessage::Pump => {}
                }
                if let Err(error) = pump_queue(&state, &active, &sender).await {
                    tracing::error!(error = %format!("{error:#}"), "tag queue pump failed");
                }
            }
            _ = interval.tick() => {
                if let Err(error) = pump_queue(&state, &active, &sender).await {
                    tracing::error!(error = %format!("{error:#}"), "tag queue pump failed");
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
    if classification_is_blocked_by_flags(settings.paused, settings.classification_paused) {
        return Ok(());
    }
    for profile in settings.profiles.into_iter().filter(classification_enabled) {
        let root = PathBuf::from(profile.output_dir);
        if root.is_dir() {
            watcher.watch(&root, RecursiveMode::Recursive)?;
            watched_roots.push(root);
        }
    }
    Ok(())
}

async fn queue_unknown_markdown(state: &AppState, active: &SharedActive) -> Result<()> {
    if classification_is_blocked(state) {
        return Ok(());
    }
    let profiles = state
        .settings
        .read()
        .await
        .profiles
        .iter()
        .filter(|profile| classification_enabled(profile))
        .cloned()
        .collect::<Vec<_>>();
    let files = tokio::task::spawn_blocking(move || -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for profile in profiles {
            files.extend(discover_markdown_files(&profile)?);
        }
        Ok(files)
    })
    .await??;
    for path in files {
        enqueue_path(state, active, path, false).await?;
    }
    Ok(())
}

async fn queue_actionable_markdown(state: &AppState, active: &SharedActive) -> Result<()> {
    if classification_is_blocked(state) {
        return Ok(());
    }
    let profiles = state
        .settings
        .read()
        .await
        .profiles
        .iter()
        .filter(|profile| classification_enabled(profile))
        .cloned()
        .collect::<Vec<_>>();
    let files = tokio::task::spawn_blocking(move || -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for profile in profiles {
            files.extend(discover_markdown_files(&profile)?);
        }
        Ok(files)
    })
    .await??;

    for path in files {
        let settings = state.settings.read().await.clone();
        let Some(profile) = matching_output_profile(&settings.profiles, &path) else {
            continue;
        };
        let current_schema = schema_hash(&profile.tagging)?;
        match state.storage.find_tag_job_by_path(&path)? {
            None => enqueue_path(state, active, path, false).await?,
            Some(job)
                if should_requeue_when_started(&job.status, &job.schema_hash, &current_schema) =>
            {
                enqueue_path(state, active, path, true).await?
            }
            Some(_) => {}
        }
    }
    Ok(())
}

async fn handle_path(
    state: &AppState,
    active: &SharedActive,
    path: PathBuf,
    force: bool,
) -> Result<()> {
    if classification_is_blocked(state) && !force {
        return Ok(());
    }
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        let settings = state.settings.read().await.clone();
        let canonical = dunce::canonicalize(&path)?;
        let Some(profile) = matching_output_profile(&settings.profiles, &canonical) else {
            return Ok(());
        };
        let profile = profile.clone();
        let files =
            tokio::task::spawn_blocking(move || discover_tree(&profile, &canonical)).await??;
        for file in files {
            enqueue_path(state, active, file, force).await?;
        }
        return Ok(());
    }
    enqueue_path(state, active, path, force).await
}

async fn enqueue_path(
    state: &AppState,
    active: &SharedActive,
    path: PathBuf,
    force: bool,
) -> Result<()> {
    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
        return Ok(());
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || !is_markdown_path(&path) {
        return Ok(());
    }
    let path = dunce::canonicalize(path)?;
    if is_excluded_markdown(&path) {
        return Ok(());
    }
    let settings = state.settings.read().await.clone();
    let Some(profile) = matching_output_profile(&settings.profiles, &path) else {
        return Ok(());
    };
    if is_profile_index(profile, &path) {
        return Ok(());
    }
    if !classification_enabled(profile) {
        return Ok(());
    }
    let output_root = dunce::canonicalize(&profile.output_dir)?;
    if !path.starts_with(&output_root) {
        return Ok(());
    }
    let relative = path
        .strip_prefix(&output_root)
        .context("Markdown 不在分类输出目录中")?;
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        anyhow::bail!("Markdown 相对路径不安全：{}", relative.display());
    }
    {
        let mut running = active.lock().await;
        if running.paths.contains(&path) {
            if force {
                running.force_after.insert(path);
            }
            return Ok(());
        }
    }
    if !force && state.storage.find_tag_job_by_path(&path)?.is_some() {
        // 已完成文档的手工修改、程序自身 YAML 写入和失败任务都不会自动重打。
        return Ok(());
    }
    let hash = schema_hash(&profile.tagging)?;
    state.storage.put_tag_job(
        &profile.id,
        &path,
        relative,
        &hash,
        TagJobStatus::Queued,
        force,
    )?;
    Ok(())
}

async fn apply_profile_rules(
    state: &AppState,
    active: &SharedActive,
    profile_id: &str,
    process_existing: bool,
) -> Result<()> {
    let settings = state.settings.read().await.clone();
    let Some(profile) = settings
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
    else {
        anyhow::bail!("找不到监控目录");
    };
    if !classification_enabled(&profile) {
        state.storage.cancel_profile_pending_tag_jobs(profile_id)?;
        return Ok(());
    }
    let hash = schema_hash(&profile.tagging)?;
    if !process_existing {
        state
            .storage
            .mark_profile_tag_jobs_outdated(profile_id, &hash)?;
    }
    let files = tokio::task::spawn_blocking({
        let profile = profile.clone();
        move || discover_markdown_files(&profile)
    })
    .await??;
    let root = dunce::canonicalize(&profile.output_dir)?;
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .context("Markdown 不在输出目录中")?;
        if process_existing {
            let mut running = active.lock().await;
            if running.paths.contains(&path) {
                running.force_after.insert(path.clone());
                continue;
            }
            drop(running);
            state.storage.put_tag_job(
                &profile.id,
                &path,
                relative,
                &hash,
                TagJobStatus::Queued,
                true,
            )?;
        } else if state.storage.find_tag_job_by_path(&path)?.is_none() {
            // 建立“仅未来文件”的基线，防止重启后的首次扫描把历史文档排队。
            state.storage.put_tag_job(
                &profile.id,
                &path,
                relative,
                &hash,
                TagJobStatus::Cancelled,
                false,
            )?;
        }
    }
    Ok(())
}

async fn retry_jobs(state: &AppState, active: &SharedActive, job_ids: &[String]) -> Result<()> {
    for id in job_ids {
        let Some(job) = state.storage.get_tag_job(id)? else {
            continue;
        };
        enqueue_path(state, active, PathBuf::from(job.markdown_path), true).await?;
    }
    Ok(())
}

async fn pump_queue(
    state: &AppState,
    active: &SharedActive,
    sender: &mpsc::UnboundedSender<TagRuntimeMessage>,
) -> Result<()> {
    if classification_is_blocked(state) {
        return Ok(());
    }
    let settings = state.settings.read().await.clone();
    if !settings.agent.configured
        || settings.agent.model.trim().is_empty()
        || settings.agent.base_url.trim().is_empty()
    {
        return Ok(());
    }
    let api_key = AppState::read_agent_api_key()?;
    let concurrency = settings.agent.concurrency.clamp(1, 4) as usize;
    let available = concurrency.saturating_sub(active.lock().await.paths.len());
    if available == 0 {
        return Ok(());
    }
    let queued = state
        .storage
        .list_tag_jobs_with_statuses(&[TagJobStatus::Queued])?;
    let mut started = 0;
    for job in queued {
        if started >= available {
            break;
        }
        let Some(profile) = settings
            .profiles
            .iter()
            .find(|profile| profile.id == job.profile_id && classification_enabled(profile))
        else {
            state.storage.set_tag_job_status(
                &job.id,
                TagJobStatus::Cancelled,
                Some("目录文档分类功能已关闭"),
            )?;
            continue;
        };
        let current_schema = schema_hash(&profile.tagging)?;
        if current_schema != job.schema_hash {
            state.storage.set_tag_job_status(
                &job.id,
                TagJobStatus::Outdated,
                Some("分类规则已变化"),
            )?;
            continue;
        }
        let path = PathBuf::from(&job.markdown_path);
        if !path.is_file() {
            state.storage.set_tag_job_status(
                &job.id,
                TagJobStatus::Cancelled,
                Some("Markdown 已不存在"),
            )?;
            continue;
        }
        {
            let mut running = active.lock().await;
            if !running.paths.insert(path.clone()) {
                continue;
            }
        }
        state
            .storage
            .set_tag_job_status(&job.id, TagJobStatus::Reading, None)?;
        started += 1;
        let state = state.clone();
        let active = active.clone();
        let sender = sender.clone();
        let job_id = job.id.clone();
        let captured_schema = job.schema_hash.clone();
        let config = profile.tagging.clone();
        let model = settings.agent.clone();
        let api_key = api_key.clone();
        tauri::async_runtime::spawn(async move {
            let result = run_tag_agent(
                state.storage.clone(),
                job_id.clone(),
                path.clone(),
                config,
                model,
                api_key,
            )
            .await;
            match result {
                Ok(result) => {
                    let result_json = serde_json::to_string(&result.categories)
                        .unwrap_or_else(|_| "[]".to_string());
                    if let Err(error) = state.storage.complete_tag_job(
                        &job_id,
                        &result.content_hash,
                        &result_json,
                        result.read_bytes,
                        result.total_bytes,
                        result.api_calls,
                        result.input_tokens,
                        result.output_tokens,
                    ) {
                        tracing::error!(job_id = %job_id, error = %format!("{error:#}"), "failed to complete tag job");
                    } else {
                        let current = state
                            .settings
                            .read()
                            .await
                            .profiles
                            .iter()
                            .find(|profile| profile.id == job.profile_id)
                            .and_then(|profile| schema_hash(&profile.tagging).ok());
                        if current.as_deref() != Some(captured_schema.as_str()) {
                            let _ = state.storage.set_tag_job_status(
                                &job_id,
                                TagJobStatus::Outdated,
                                Some("任务执行期间分类规则发生变化"),
                            );
                        }
                    }
                }
                Err(error) => {
                    let _ = state.storage.set_tag_job_status(
                        &job_id,
                        TagJobStatus::Failed,
                        Some(&format!("{error:#}")),
                    );
                }
            }
            let pending_force = {
                let mut running = active.lock().await;
                running.paths.remove(&path);
                running.force_after.remove(&path)
            };
            if pending_force {
                let _ = sender.send(TagRuntimeMessage::Path { path, force: true });
            } else {
                let _ = sender.send(TagRuntimeMessage::Pump);
            }
        });
    }
    Ok(())
}

pub fn discover_markdown_files(profile: &WatchProfile) -> Result<Vec<PathBuf>> {
    let root = dunce::canonicalize(&profile.output_dir)
        .with_context(|| format!("无法读取输出目录：{}", profile.output_dir))?;
    discover_tree(profile, &root)
}

fn discover_tree(profile: &WatchProfile, start: &Path) -> Result<Vec<PathBuf>> {
    let root = dunce::canonicalize(&profile.output_dir)?;
    let start = dunce::canonicalize(start)?;
    if !start.starts_with(&root) {
        anyhow::bail!("分类扫描目录不在输出范围内：{}", start.display());
    }
    let mut files = Vec::new();
    for entry in WalkDir::new(start).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() || !entry.file_type().is_file() {
            continue;
        }
        if !is_markdown_path(entry.path())
            || is_excluded_markdown(entry.path())
            || is_profile_index(profile, entry.path())
        {
            continue;
        }
        let path = dunce::canonicalize(entry.path())?;
        if path.starts_with(&root) {
            files.push(path);
        }
    }
    Ok(files)
}

fn matching_output_profile<'a>(
    profiles: &'a [WatchProfile],
    path: &Path,
) -> Option<&'a WatchProfile> {
    profiles
        .iter()
        .filter(|profile| classification_enabled(profile))
        .filter(|profile| path.starts_with(&profile.output_dir))
        .max_by_key(|profile| profile.output_dir.len())
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn classification_enabled(profile: &WatchProfile) -> bool {
    profile.enabled && profile.tagging.enabled && !profile.tagging.labels.is_empty()
}

fn classification_is_blocked(state: &AppState) -> bool {
    state.is_classification_paused()
}

fn classification_is_blocked_by_flags(
    _conversion_paused: bool,
    classification_paused: bool,
) -> bool {
    classification_paused
}

fn should_requeue_when_started(
    status: &TagJobStatus,
    job_schema: &str,
    current_schema: &str,
) -> bool {
    match status {
        TagJobStatus::Reading | TagJobStatus::Writing => false,
        TagJobStatus::Completed | TagJobStatus::Queued => job_schema != current_schema,
        TagJobStatus::Failed | TagJobStatus::Outdated | TagJobStatus::Cancelled => true,
    }
}

fn is_excluded_markdown(path: &Path) -> bool {
    if path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|part| part.eq_ignore_ascii_case(".trash"))
    }) {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    name.ends_with(".tmp") || name.contains(".cpah.tmp") || name.starts_with("~$")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_excludes_trash_temporary_and_symlink_targets() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(output.join("nested")).unwrap();
        std::fs::create_dir_all(output.join(".trash")).unwrap();
        std::fs::write(output.join("nested/a.md"), "a").unwrap();
        std::fs::write(output.join("index.md"), "generated index").unwrap();
        std::fs::write(output.join(".trash/b.md"), "b").unwrap();
        std::fs::write(output.join("nested/c.md.cpah.tmp"), "c").unwrap();
        let input = dunce::canonicalize(input).unwrap();
        let output = dunce::canonicalize(output).unwrap();
        let profile = WatchProfile {
            id: "p".into(),
            name: "p".into(),
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            enabled: true,
            delete_policy: Default::default(),
            tagging: Default::default(),
        };
        let files = discover_markdown_files(&profile).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("a.md"));
    }

    #[test]
    fn legacy_field_configuration_is_inert_until_categories_are_configured() {
        let tagging: crate::models::TaggingConfig = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "fields": [{
                "id": "topics",
                "name": "topics",
                "description": "旧字段",
                "valueType": "list"
            }]
        }))
        .unwrap();
        let profile = WatchProfile {
            id: "legacy".into(),
            name: "legacy".into(),
            input_dir: "input".into(),
            output_dir: "output".into(),
            enabled: true,
            delete_policy: Default::default(),
            tagging,
        };
        assert_eq!(
            profile.tagging.selection_mode,
            crate::models::TagSelectionMode::Single
        );
        assert!(profile.tagging.labels.is_empty());
        assert!(!classification_enabled(&profile));
    }

    #[test]
    fn start_requeues_only_actionable_classification_jobs() {
        for status in [
            TagJobStatus::Failed,
            TagJobStatus::Outdated,
            TagJobStatus::Cancelled,
        ] {
            assert!(should_requeue_when_started(&status, "same", "same"));
        }
        for status in [TagJobStatus::Reading, TagJobStatus::Writing] {
            assert!(!should_requeue_when_started(&status, "old", "new"));
        }
        assert!(!should_requeue_when_started(
            &TagJobStatus::Completed,
            "same",
            "same"
        ));
        assert!(!should_requeue_when_started(
            &TagJobStatus::Queued,
            "same",
            "same"
        ));
        assert!(should_requeue_when_started(
            &TagJobStatus::Completed,
            "old",
            "new"
        ));
        assert!(should_requeue_when_started(
            &TagJobStatus::Queued,
            "old",
            "new"
        ));
    }

    #[test]
    fn only_classification_pause_blocks_classification_runtime() {
        assert!(!classification_is_blocked_by_flags(false, false));
        assert!(!classification_is_blocked_by_flags(true, false));
        assert!(classification_is_blocked_by_flags(false, true));
        assert!(classification_is_blocked_by_flags(true, true));
    }
}
