use crate::converter::is_supported;
use crate::diagnostics;
use crate::index_runtime::IndexRuntimeMessage;
use crate::models::{
    AgentSettings, AppSettings, Dashboard, HealthReport, JobStatus, TagJobRecord, TagJobStatus,
    TaggingConfig, TaggingImpact,
};
use crate::runtime::RuntimeMessage;
use crate::state::AppState;
use crate::tag_runtime::{self, TagRuntimeMessage};
use crate::tagging::{schema_hash, test_tool_calling, validate_tagging_config};
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
        .list_tasks(DASHBOARD_RECORD_LIMIT)
        .map_err(display_error)?;
    let tag_jobs = state
        .storage
        .list_tag_jobs(DASHBOARD_RECORD_LIMIT)
        .map_err(display_error)?;
    let task_total = state.storage.task_count().map_err(display_error)?;
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
    let tasks = state
        .storage
        .list_tasks_with_statuses(&[JobStatus::Failed])
        .map_err(display_error)?;
    let mut queued = 0;
    for task in tasks {
        if Path::new(&task.source_path).is_file() {
            state
                .send_runtime(RuntimeMessage::Retry { task_id: task.id })
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
    mut settings: AppSettings,
) -> CommandResult<AppSettings> {
    let current = state.settings.read().await.clone();
    // Agent 连接只允许通过凭据命令修改。分类规则本身可以安全自动保存；
    // 保存后只建立“从现在开始”的基线，不会把历史 Markdown 批量送给模型。
    validate_settings(&mut settings).map_err(display_error)?;
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
    {
        let mut live = state.settings.write().await;
        // 这些状态只能由各自的专用命令修改。持有写锁完成落盘，避免目录自动保存与按钮操作并发时互相覆盖。
        settings.agent = live.agent.clone();
        settings.monitoring_paused = live.monitoring_paused;
        settings.paused = live.paused;
        settings.classification_paused = live.classification_paused;
        state
            .storage
            .save_settings(&settings)
            .map_err(display_error)?;
        *live = settings.clone();
    }
    state.set_monitoring_paused_flag(settings.monitoring_paused);
    state.set_paused_flag(settings.paused);
    state
        .storage
        .delete_profile_records(&removed_profile_ids)
        .map_err(display_error)?;
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

fn validate_agent_base_url(value: &str) -> anyhow::Result<String> {
    let value = value.trim();
    let parsed = reqwest::Url::parse(value).map_err(|_| anyhow::anyhow!("Base URL 格式无效"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("Base URL 必须以 http:// 或 https:// 开头");
    }
    if parsed.host_str().is_none() {
        anyhow::bail!("Base URL 必须包含有效主机名");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        anyhow::bail!("Base URL 不允许包含用户名或密码");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        anyhow::bail!("Base URL 不允许包含查询参数或片段");
    }
    Ok(value.trim_end_matches('/').to_string())
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
    use super::validate_agent_base_url;

    #[test]
    fn validates_agent_base_urls_strictly() {
        assert_eq!(
            validate_agent_base_url(" https://example.com/v1/ ").unwrap(),
            "https://example.com/v1"
        );
        assert!(validate_agent_base_url("file:///tmp/api").is_err());
        assert!(validate_agent_base_url("https://user:secret@example.com/v1").is_err());
        assert!(validate_agent_base_url("https://example.com/v1?key=secret").is_err());
        assert!(validate_agent_base_url("not a url").is_err());
    }
}
