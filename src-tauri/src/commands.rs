use crate::converter::is_supported;
use crate::diagnostics;
use crate::index_runtime::IndexRuntimeMessage;
use crate::models::{
    AgentSettings, AppSettings, Dashboard, HealthReport, JobStatus, TagJobRecord, TagJobStatus,
    TaggingConfig, TaggingImpact,
};
use crate::runtime::RuntimeMessage;
use crate::state::{AppState, ProfileRuntimeControl};
use crate::tag_runtime::{self, TagRuntimeMessage};
use crate::tagging::{
    schema_hash, test_tool_calling, validate_agent_base_url, validate_tagging_config,
};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

type CommandResult<T> = std::result::Result<T, String>;

const MINERU_TOKEN_PAGE_URL: &str = "https://mineru.net/apiManage/token";

#[tauri::command]
pub async fn get_dashboard(state: State<'_, AppState>) -> CommandResult<Dashboard> {
    const DASHBOARD_RECORD_LIMIT: usize = 5_000;
    let mut tasks = state
        .storage
        .list_visible_tasks(DASHBOARD_RECORD_LIMIT)
        .map_err(display_error)?;
    let tag_jobs = state
        .storage
        .list_tag_jobs(DASHBOARD_RECORD_LIMIT)
        .map_err(display_error)?;
    let task_total = state.storage.visible_task_count().map_err(display_error)?;
    let tag_job_total = state.storage.tag_job_count().map_err(display_error)?;
    let by_output = tag_jobs
        .iter()
        .map(|job| (normalized_path_key(&job.markdown_path), job))
        .collect::<HashMap<_, _>>();
    for task in &mut tasks {
        if let Some(job) = task
            .output_path
            .as_deref()
            .and_then(|path| by_output.get(&normalized_path_key(path)))
        {
            task.tag_job_id = Some(job.id.clone());
            task.tag_status = Some(job.status.clone());
        }
    }
    Ok(Dashboard {
        settings: state.settings.read().await.clone(),
        tasks,
        tag_jobs,
        task_total,
        tag_job_total,
        runtime_error: state.runtime_error(),
        tag_runtime_error: state.tag_runtime_error(),
        index_runtime_error: state.index_runtime_error(),
    })
}

#[tauri::command]
pub async fn run_health_check(state: State<'_, AppState>) -> CommandResult<HealthReport> {
    Ok(diagnostics::run_health_check(&state).await)
}

#[tauri::command]
pub async fn get_diagnostic_report(state: State<'_, AppState>) -> CommandResult<String> {
    diagnostics::diagnostic_report(&state)
        .await
        .map_err(display_error)
}

#[tauri::command]
pub fn get_project_license() -> String {
    include_str!("../../LICENSE").to_string()
}

#[tauri::command]
pub fn get_third_party_licenses() -> String {
    include_str!("../../THIRD_PARTY_LICENSES.md").to_string()
}

#[tauri::command]
pub fn rescan_all_profiles(state: State<'_, AppState>) -> CommandResult<()> {
    if state.is_monitoring_paused() {
        return Err("目录监听已停止，请先点击“开始监听”再重新扫描目录".to_string());
    }
    tracing::info!("manual directory rescan requested");
    state
        .send_runtime(RuntimeMessage::Reconcile)
        .map_err(display_error)
}

#[tauri::command]
pub fn retry_failed_tasks(state: State<'_, AppState>) -> CommandResult<usize> {
    let parent_tasks = state
        .storage
        .list_tasks_with_statuses(&[JobStatus::Failed])
        .map_err(display_error)?;
    let part_tasks = state
        .storage
        .list_mineru_parts_with_statuses(&[JobStatus::Failed])
        .map_err(display_error)?;
    let mut queued = 0;
    for task in parent_tasks {
        if Path::new(&task.source_path).is_file() {
            state
                .send_runtime(RuntimeMessage::Retry { task_id: task.id })
                .map_err(display_error)?;
            queued += 1;
        }
    }
    for part in part_tasks {
        let source_exists = state
            .storage
            .get_task(&part.parent_task_id)
            .map_err(display_error)?
            .is_some_and(|task| Path::new(&task.source_path).is_file());
        if source_exists {
            state
                .send_runtime(RuntimeMessage::Retry { task_id: part.id })
                .map_err(display_error)?;
            queued += 1;
        }
    }
    tracing::info!(count = queued, "failed conversions queued for retry");
    Ok(queued)
}

#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
    expected_profile_ids: Vec<String>,
) -> CommandResult<AppSettings> {
    let (settings, removed_profile_ids, controls, tag_baselines) =
        save_settings_core(&state, settings, &expected_profile_ids).await?;
    state.set_monitoring_paused_flag(settings.monitoring_paused);
    state.set_paused_flag(settings.paused);
    cleanup_removed_profiles(&state, &removed_profile_ids, controls).await?;
    state
        .storage
        .delete_disabled_waiting_tasks(&settings.enabled_extensions)
        .map_err(display_error)?;
    state
        .send_runtime(RuntimeMessage::Reload)
        .map_err(display_error)?;
    for profile_id in tag_baselines {
        state
            .send_tag_runtime(TagRuntimeMessage::ApplyRules {
                profile_id,
                process_existing: false,
            })
            .map_err(display_error)?;
    }
    state
        .send_tag_runtime(TagRuntimeMessage::Reload)
        .map_err(display_error)?;
    state
        .send_index_runtime(IndexRuntimeMessage::Reload)
        .map_err(display_error)?;
    Ok(settings)
}

async fn save_settings_core(
    state: &AppState,
    mut settings: AppSettings,
    expected_profile_ids: &[String],
) -> CommandResult<(
    AppSettings,
    Vec<String>,
    Vec<ProfileRuntimeControl>,
    Vec<String>,
)> {
    // Agent 连接只允许通过凭据命令修改。分类规则本身可以安全自动保存；
    // 保存后只建立“从现在开始”的基线，不会把历史 Markdown 批量送给模型。
    validate_settings(&mut settings).map_err(display_error)?;
    let mut live = state.settings.write().await;
    let current = live.clone();
    let actual_ids = current
        .profiles
        .iter()
        .map(|profile| profile.id.as_str())
        .collect::<HashSet<_>>();
    let expected_ids = expected_profile_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if actual_ids != expected_ids || expected_ids.len() != expected_profile_ids.len() {
        return Err("监控目录已在其他窗口变更，请重新载入后再保存".to_string());
    }
    let removed_profile_ids = current
        .profiles
        .iter()
        .filter(|profile| !settings.profiles.iter().any(|item| item.id == profile.id))
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();
    let tag_baselines = settings
        .profiles
        .iter()
        .filter(|profile| {
            current
                .profiles
                .iter()
                .find(|item| item.id == profile.id)
                .is_none_or(|existing| {
                    existing.output_dir != profile.output_dir
                        || existing.enabled != profile.enabled
                        || existing.tagging != profile.tagging
                })
        })
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();
    settings.mineru_configured =
        AppState::read_mineru_token().is_ok_and(|token| !token.trim().is_empty());
    // 保存、控制对象切换和取消必须在同一设置写锁内完成。
    settings.agent = live.agent.clone();
    settings.monitoring_paused = live.monitoring_paused;
    settings.paused = live.paused;
    settings.classification_paused = live.classification_paused;
    state
        .storage
        .save_settings(&settings)
        .map_err(display_error)?;
    *live = settings.clone();
    state.ensure_profile_controls(settings.profiles.iter().map(|profile| profile.id.as_str()));
    let controls = removed_profile_ids
        .iter()
        .filter_map(|profile_id| state.begin_profile_removal(profile_id))
        .collect();
    Ok((settings, removed_profile_ids, controls, tag_baselines))
}

#[tauri::command]
pub async fn remove_profile(
    state: State<'_, AppState>,
    profile_id: String,
) -> CommandResult<AppSettings> {
    let settings = remove_profile_core(&state, &profile_id).await?;
    state
        .send_runtime(RuntimeMessage::Reload)
        .map_err(display_error)?;
    state
        .send_tag_runtime(TagRuntimeMessage::Reload)
        .map_err(display_error)?;
    state
        .send_index_runtime(IndexRuntimeMessage::Reload)
        .map_err(display_error)?;
    tracing::info!(profile_id = %profile_id, "watch profile removed and active work cancelled");
    Ok(settings)
}

async fn remove_profile_core(state: &AppState, profile_id: &str) -> CommandResult<AppSettings> {
    let (settings, controls) = {
        let mut live = state.settings.write().await;
        if !live.profiles.iter().any(|profile| profile.id == profile_id) {
            return Err("找不到监控目录".to_string());
        }
        let mut updated = live.clone();
        updated.profiles.retain(|profile| profile.id != profile_id);
        state
            .storage
            .save_settings(&updated)
            .map_err(display_error)?;
        *live = updated.clone();
        let controls = state
            .begin_profile_removal(profile_id)
            .into_iter()
            .collect();
        (updated, controls)
    };
    cleanup_removed_profiles(state, &[profile_id.to_string()], controls).await?;
    Ok(settings)
}

async fn cleanup_removed_profiles(
    state: &AppState,
    profile_ids: &[String],
    controls: Vec<ProfileRuntimeControl>,
) -> CommandResult<()> {
    for control in controls {
        control.wait_for_writers().await;
    }
    let work_dirs = profile_ids
        .iter()
        .map(|profile_id| state.storage.list_profile_tasks(profile_id))
        .collect::<Result<Vec<_>, _>>()
        .map_err(display_error)?
        .into_iter()
        .flatten()
        .map(|task| state.storage.mineru_work_root().join(task.id))
        .collect::<Vec<_>>();
    state
        .storage
        .delete_profile_records(profile_ids)
        .map_err(display_error)?;
    if let Err(error) = tokio::task::spawn_blocking(move || {
        for work_dir in work_dirs {
            if work_dir.exists()
                && let Err(error) = std::fs::remove_dir_all(&work_dir)
            {
                tracing::warn!(path = %work_dir.display(), error = %error, "failed to clean removed profile MinerU cache");
            }
        }
    })
    .await
    {
        tracing::warn!(error = %error, "removed profile MinerU cache cleanup task failed");
    }
    for profile_id in profile_ids {
        state.forget_cancelled_profile(profile_id);
    }
    Ok(())
}

#[tauri::command]
pub async fn open_managed_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<()> {
    let target = dunce::canonicalize(Path::new(&path)).map_err(display_error)?;
    let settings = state.settings.read().await;
    let allowed = settings.profiles.iter().any(|profile| {
        [&profile.input_dir, &profile.output_dir]
            .into_iter()
            .filter_map(|root| dunce::canonicalize(root).ok())
            .any(|root| target.starts_with(root))
    });
    if !allowed {
        return Err(format!("拒绝打开未配置目录中的路径：{}", target.display()));
    }
    app.opener()
        .open_path(target.to_string_lossy().into_owned(), None::<String>)
        .map_err(display_error)
}

#[tauri::command]
pub fn open_mineru_token_page(app: AppHandle) -> CommandResult<()> {
    app.opener()
        .open_url(MINERU_TOKEN_PAGE_URL, None::<String>)
        .map_err(display_error)
}

#[tauri::command]
pub async fn set_mineru_token(state: State<'_, AppState>, token: String) -> CommandResult<()> {
    let token = token.trim();
    if token.is_empty() {
        return Err("MinerU Token 不能为空".to_string());
    }
    AppState::write_mineru_token(token).map_err(display_error)?;
    {
        let mut settings = state.settings.write().await;
        settings.mineru_configured = true;
        state
            .storage
            .save_settings(&settings)
            .map_err(display_error)?;
    }
    state
        .send_runtime(RuntimeMessage::RetryWaitingMineru)
        .map_err(display_error)
}

#[tauri::command]
pub async fn set_paused(state: State<'_, AppState>, paused: bool) -> CommandResult<AppSettings> {
    let settings = {
        let mut live = state.settings.write().await;
        let mut updated = live.clone();
        updated.paused = paused;
        state
            .storage
            .save_settings(&updated)
            .map_err(display_error)?;
        *live = updated.clone();
        updated
    };
    state.set_paused_flag(paused);
    if !paused {
        state
            .send_runtime(RuntimeMessage::ProcessQueued)
            .map_err(display_error)?;
    }
    Ok(settings)
}

#[tauri::command]
pub async fn set_monitoring_paused(
    state: State<'_, AppState>,
    paused: bool,
) -> CommandResult<AppSettings> {
    let settings = {
        let mut live = state.settings.write().await;
        let mut updated = live.clone();
        updated.monitoring_paused = paused;
        state
            .storage
            .save_settings(&updated)
            .map_err(display_error)?;
        *live = updated.clone();
        updated
    };
    state.set_monitoring_paused_flag(paused);
    state
        .send_runtime(RuntimeMessage::Reload)
        .map_err(display_error)?;
    Ok(settings)
}

#[tauri::command]
pub async fn set_classification_paused(
    state: State<'_, AppState>,
    paused: bool,
) -> CommandResult<AppSettings> {
    let settings = {
        let mut live = state.settings.write().await;
        let mut updated = live.clone();
        updated.classification_paused = paused;
        state
            .storage
            .save_settings(&updated)
            .map_err(display_error)?;
        *live = updated.clone();
        updated
    };
    state.set_classification_paused_flag(paused);
    state
        .send_tag_runtime(if paused {
            TagRuntimeMessage::Reload
        } else {
            TagRuntimeMessage::Start
        })
        .map_err(display_error)?;
    Ok(settings)
}

#[tauri::command]
pub async fn save_agent_settings(
    state: State<'_, AppState>,
    base_url: String,
    model: String,
    api_key: Option<String>,
    concurrency: u8,
) -> CommandResult<AgentSettings> {
    let base_url = validate_agent_base_url(&base_url).map_err(display_error)?;
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err("模型名称不能为空".to_string());
    }
    if !(1..=4).contains(&concurrency) {
        return Err("Agent 并发数必须在 1–4 之间".to_string());
    }
    if let Some(api_key) = api_key.as_deref().map(str::trim)
        && !api_key.is_empty()
    {
        AppState::write_agent_api_key(api_key).map_err(display_error)?;
    }
    let configured = AppState::read_agent_api_key().is_ok_and(|key| !key.trim().is_empty());
    let agent = AgentSettings {
        base_url,
        model,
        configured,
        concurrency,
    };
    {
        let mut settings = state.settings.write().await;
        settings.agent = agent.clone();
        state
            .storage
            .save_settings(&settings)
            .map_err(display_error)?;
    }
    state
        .send_tag_runtime(TagRuntimeMessage::Reload)
        .map_err(display_error)?;
    Ok(agent)
}

#[tauri::command]
pub async fn test_agent_connection(
    _state: State<'_, AppState>,
    base_url: String,
    model: String,
    api_key: Option<String>,
) -> CommandResult<()> {
    let settings = AgentSettings {
        base_url: validate_agent_base_url(&base_url).map_err(display_error)?,
        model: model.trim().to_string(),
        configured: true,
        concurrency: 1,
    };
    if settings.model.is_empty() {
        return Err("模型名称不能为空".to_string());
    }
    let api_key = match api_key.as_deref().map(str::trim) {
        Some(key) if !key.is_empty() => key.to_string(),
        _ => AppState::read_agent_api_key().map_err(display_error)?,
    };
    test_tool_calling(&settings, &api_key)
        .await
        .map_err(display_error)
}

#[tauri::command]
pub async fn preview_tagging_change(
    state: State<'_, AppState>,
    profile_id: String,
    mut tagging: TaggingConfig,
) -> CommandResult<TaggingImpact> {
    validate_tagging_config(&mut tagging).map_err(display_error)?;
    let settings = state.settings.read().await.clone();
    let profile = settings
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "找不到监控目录".to_string())?;
    let files = tokio::task::spawn_blocking(move || tag_runtime::discover_markdown_files(&profile))
        .await
        .map_err(display_error)?
        .map_err(display_error)?;
    let hash = schema_hash(&tagging).map_err(display_error)?;
    let mut new_files = 0;
    let mut affected = 0;
    for path in &files {
        match state
            .storage
            .find_tag_job_by_path(path)
            .map_err(display_error)?
        {
            None => {
                new_files += 1;
                affected += 1;
            }
            Some(job) if job.schema_hash != hash || job.status != TagJobStatus::Completed => {
                affected += 1;
            }
            Some(_) => {}
        }
    }
    Ok(TaggingImpact {
        discovered: files.len(),
        new_files,
        affected,
    })
}

#[tauri::command]
pub async fn apply_tagging_config(
    state: State<'_, AppState>,
    profile_id: String,
    mut tagging: TaggingConfig,
    process_existing: bool,
) -> CommandResult<AppSettings> {
    validate_tagging_config(&mut tagging).map_err(display_error)?;
    let settings = {
        let mut settings = state.settings.write().await;
        let profile = settings
            .profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| "找不到监控目录".to_string())?;
        profile.tagging = tagging;
        state
            .storage
            .save_settings(&settings)
            .map_err(display_error)?;
        settings.clone()
    };
    if !settings
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .is_some_and(|profile| profile.tagging.enabled)
    {
        state
            .storage
            .cancel_profile_pending_tag_jobs(&profile_id)
            .map_err(display_error)?;
    }
    state
        .send_tag_runtime(TagRuntimeMessage::ApplyRules {
            profile_id,
            process_existing,
        })
        .map_err(display_error)?;
    state
        .send_tag_runtime(TagRuntimeMessage::Reload)
        .map_err(display_error)?;
    Ok(settings)
}

#[tauri::command]
pub fn get_tag_jobs(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> CommandResult<Vec<TagJobRecord>> {
    state
        .storage
        .list_tag_jobs(limit.unwrap_or(100_000).min(100_000))
        .map_err(display_error)
}

#[tauri::command]
pub fn retry_tag_job(state: State<'_, AppState>, job_id: String) -> CommandResult<()> {
    state
        .send_tag_runtime(TagRuntimeMessage::Retry { job_id })
        .map_err(display_error)
}

#[tauri::command]
pub fn retry_tag_jobs(state: State<'_, AppState>, job_ids: Vec<String>) -> CommandResult<()> {
    state
        .send_tag_runtime(TagRuntimeMessage::RetryMany { job_ids })
        .map_err(display_error)
}

#[tauri::command]
pub async fn retry_task(
    state: State<'_, AppState>,
    task_id: String,
    force_local: bool,
) -> CommandResult<()> {
    if force_local {
        return Err(
            "当前纯 Rust 本地转换器不支持 PDF、图片、DOC 或 PPT，请使用 MinerU 重试".to_string(),
        );
    }
    state
        .send_runtime(RuntimeMessage::Retry { task_id })
        .map_err(display_error)
}

fn validate_settings(settings: &mut AppSettings) -> anyhow::Result<()> {
    let mut extensions = HashSet::new();
    settings.enabled_extensions = settings
        .enabled_extensions
        .drain(..)
        .filter_map(|extension| {
            let extension = extension
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            let probe = PathBuf::from(format!("file.{extension}"));
            (is_supported(&probe) && extensions.insert(extension.clone())).then_some(extension)
        })
        .collect();
    let mut ids = HashSet::new();
    for (index, profile) in settings.profiles.iter_mut().enumerate() {
        if profile.id.trim().is_empty() || !ids.insert(profile.id.clone()) {
            profile.id = Uuid::new_v4().to_string();
            ids.insert(profile.id.clone());
        }
        if profile.name.trim().is_empty() {
            profile.name = format!("目录 {}", index + 1);
        }
        let input = canonical_directory(Path::new(profile.input_dir.trim()), false)?;
        let output = canonical_directory(Path::new(profile.output_dir.trim()), true)?;
        if paths_overlap(&input, &output) {
            anyhow::bail!("“{}”的输入和输出目录不能互相包含", profile.name);
        }
        profile.input_dir = input.to_string_lossy().to_string();
        profile.output_dir = output.to_string_lossy().to_string();
        validate_tagging_config(&mut profile.tagging)?;
    }

    for left in 0..settings.profiles.len() {
        for right in (left + 1)..settings.profiles.len() {
            let a = &settings.profiles[left];
            let b = &settings.profiles[right];
            let a_input = Path::new(&a.input_dir);
            let a_output = Path::new(&a.output_dir);
            let b_input = Path::new(&b.input_dir);
            let b_output = Path::new(&b.output_dir);
            if paths_overlap(a_input, b_input) {
                anyhow::bail!("监控目录“{}”与“{}”互相重叠", a.name, b.name);
            }
            if paths_overlap(a_input, b_output) || paths_overlap(b_input, a_output) {
                anyhow::bail!("“{}”与“{}”的输入、输出目录存在交叉", a.name, b.name);
            }
            if paths_overlap(a_output, b_output) {
                anyhow::bail!("“{}”与“{}”的输出目录不能互相重叠", a.name, b.name);
            }
        }
    }
    if settings.mineru_base_url.trim().is_empty() {
        anyhow::bail!("MinerU API 地址不能为空");
    }
    settings.mineru_base_url = settings.mineru_base_url.trim_end_matches('/').to_string();
    settings.agent.concurrency = settings.agent.concurrency.clamp(1, 4);
    Ok(())
}

fn normalized_path_key(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\").to_lowercase()
    } else {
        path.to_string()
    }
}

fn canonical_directory(path: &Path, create: bool) -> anyhow::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        anyhow::bail!("输入和输出目录都必须选择");
    }
    if create {
        std::fs::create_dir_all(path)?;
    }
    if !path.is_dir() {
        anyhow::bail!("目录不存在：{}", path.display());
    }
    Ok(dunce::canonicalize(path)?)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::{remove_profile_core, save_settings_core};
    use crate::models::{ConversionEngine, MinerUPartMode, TagJobStatus, WatchProfile};
    use crate::pdf_split::PdfPartPlan;
    use crate::state::AppState;
    use crate::tagging::validate_agent_base_url;
    use std::path::Path;

    #[test]
    fn validates_agent_base_urls_strictly() {
        assert_eq!(
            validate_agent_base_url(" https://example.com/v1/ ").unwrap(),
            "https://example.com/v1"
        );
        assert_eq!(
            validate_agent_base_url("http://127.0.0.1:11434/v1/").unwrap(),
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(
            validate_agent_base_url("http://[::1]:11434/v1").unwrap(),
            "http://[::1]:11434/v1"
        );
        assert!(validate_agent_base_url("http://example.com/v1").is_err());
        assert!(validate_agent_base_url("http://192.168.1.2:11434/v1").is_err());
        assert!(validate_agent_base_url("file:///tmp/api").is_err());
        assert!(validate_agent_base_url("https://user:secret@example.com/v1").is_err());
        assert!(validate_agent_base_url("https://example.com/v1?key=secret").is_err());
        assert!(validate_agent_base_url("not a url").is_err());
    }

    #[tokio::test]
    async fn removing_profile_persists_config_and_records_without_deleting_files() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        let source = input.join("notes.md");
        let generated = output.join("notes.md");
        std::fs::write(&source, b"# source").unwrap();
        std::fs::write(&generated, b"# generated").unwrap();
        let profile = WatchProfile {
            id: "remove-me".to_string(),
            name: "Remove me".to_string(),
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            enabled: true,
            delete_policy: Default::default(),
            tagging: Default::default(),
        };
        let state = AppState::new(temporary.path().join("data")).unwrap();
        {
            let mut settings = state.settings.write().await;
            settings.profiles = vec![profile.clone()];
            state.storage.save_settings(&settings).unwrap();
        }
        state.ensure_profile_controls([profile.id.as_str()]);
        state
            .storage
            .queue_task(
                &profile,
                &source,
                Path::new("notes.md"),
                8,
                1,
                ConversionEngine::Anytomd,
                &generated,
                false,
            )
            .unwrap();
        state
            .storage
            .put_tag_job(
                &profile.id,
                &generated,
                Path::new("notes.md"),
                "schema",
                TagJobStatus::Queued,
                true,
            )
            .unwrap();

        let saved = remove_profile_core(&state, &profile.id).await.unwrap();

        assert!(saved.profiles.is_empty());
        assert!(state.storage.load_settings().unwrap().profiles.is_empty());
        assert_eq!(state.storage.task_count().unwrap(), 0);
        assert_eq!(state.storage.tag_job_count().unwrap(), 0);
        assert!(source.exists());
        assert!(generated.exists());
        assert!(state.profile_control(&profile.id).is_none());
    }

    #[tokio::test]
    async fn stale_settings_cannot_restore_a_removed_profile() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        let profile = WatchProfile {
            id: "stale-profile".to_string(),
            name: "Stale".to_string(),
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            enabled: true,
            delete_policy: Default::default(),
            tagging: Default::default(),
        };
        let state = AppState::new(temporary.path().join("data")).unwrap();
        let stale_settings = {
            let mut settings = state.settings.write().await;
            settings.profiles.push(profile.clone());
            state.storage.save_settings(&settings).unwrap();
            settings.clone()
        };
        state.ensure_profile_controls([profile.id.as_str()]);

        remove_profile_core(&state, &profile.id).await.unwrap();
        let error = save_settings_core(&state, stale_settings, &[profile.id.clone()])
            .await
            .err()
            .unwrap();
        assert!(error.contains("其他窗口变更"));
        assert!(state.settings.read().await.profiles.is_empty());
        assert!(state.storage.load_settings().unwrap().profiles.is_empty());
    }

    #[tokio::test]
    async fn deleting_profile_waits_for_pdf_writer_and_cleans_parts_and_cache() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        let source = input.join("large.pdf");
        let generated = output.join("large.md");
        std::fs::write(&source, b"pdf").unwrap();
        std::fs::write(&generated, b"existing result").unwrap();
        let profile = WatchProfile {
            id: "pdf-profile".to_string(),
            name: "PDF".to_string(),
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            enabled: true,
            delete_policy: Default::default(),
            tagging: Default::default(),
        };
        let state = AppState::new(temporary.path().join("data")).unwrap();
        {
            let mut settings = state.settings.write().await;
            settings.profiles.push(profile.clone());
            state.storage.save_settings(&settings).unwrap();
        }
        state.ensure_profile_controls([profile.id.as_str()]);
        let parent = state
            .storage
            .prepare_task(
                &profile,
                &source,
                Path::new("large.pdf"),
                "hash",
                3,
                1,
                ConversionEngine::Mineru,
                &generated,
                true,
            )
            .unwrap()
            .unwrap();
        state
            .storage
            .replace_mineru_parts(
                &parent.id,
                "hash",
                1,
                &[PdfPartPlan {
                    index: 1,
                    count: 1,
                    page_start: 1,
                    page_end: 1,
                    mode: MinerUPartMode::SplitPdf,
                    input_path: None,
                }],
            )
            .unwrap();
        let work_dir = state.storage.mineru_work_root().join(&parent.id);
        std::fs::create_dir_all(&work_dir).unwrap();
        std::fs::write(work_dir.join("part.pdf"), b"part").unwrap();

        let control = state.profile_control(&profile.id).unwrap();
        let writer = control.write_permit().await.unwrap();
        let state_for_delete = state.clone();
        let profile_id = profile.id.clone();
        let deleting =
            tokio::spawn(async move { remove_profile_core(&state_for_delete, &profile_id).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), control.cancelled())
            .await
            .unwrap();
        assert!(!deleting.is_finished());
        assert!(work_dir.exists());
        drop(writer);
        deleting.await.unwrap().unwrap();

        assert!(control.write_permit().await.is_err());
        assert_eq!(state.storage.visible_task_count().unwrap(), 0);
        assert!(!work_dir.exists());
        assert_eq!(std::fs::read(&generated).unwrap(), b"existing result");
        assert!(source.exists());
    }
}
