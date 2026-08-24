use crate::models::{
    AppSettings, HealthCheck, HealthCounts, HealthLevel, HealthReport, JobStatus, TagJobStatus,
    WatchProfile,
};
use crate::state::AppState;
use anyhow::Result;
use chrono::Utc;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

pub struct ErrorGuidance {
    pub code: &'static str,
    pub title: &'static str,
    pub suggestion: &'static str,
}

pub fn classify_error(message: Option<&str>) -> Option<ErrorGuidance> {
    let message = message?.to_ascii_lowercase();
    let guidance = if contains_any(&message, &["未配置 mineru", "mineru token", "凭据库"]) {
        ErrorGuidance {
            code: "mineru_not_configured",
            title: "MinerU 尚未配置",
            suggestion: "请在设置中保存 MinerU Token，然后重新提交任务。",
        }
    } else if contains_any(
        &message,
        &["429", "too many requests", "rate limit", "限流"],
    ) {
        ErrorGuidance {
            code: "service_rate_limited",
            title: "云端服务请求过多",
            suggestion: "请稍后重试；若持续出现，可降低并发或检查服务配额。",
        }
    } else if contains_any(&message, &["timeout", "timed out", "超时"]) {
        ErrorGuidance {
            code: "network_timeout",
            title: "网络请求超时",
            suggestion: "请检查网络和服务地址，稍后重试任务。",
        }
    } else if contains_any(
        &message,
        &["500", "502", "503", "504", "server error", "服务错误"],
    ) {
        ErrorGuidance {
            code: "service_unavailable",
            title: "云端服务暂时不可用",
            suggestion: "这通常是服务端临时故障，请稍后重试。",
        }
    } else if contains_any(
        &message,
        &["tool calling", "tool_call", "未调用分类工具", "不支持工具"],
    ) {
        ErrorGuidance {
            code: "agent_tool_calling",
            title: "模型未正确调用分类工具",
            suggestion: "请确认模型支持 Chat Completions Tool Calling，并在设置中重新测试连接。",
        }
    } else if contains_any(
        &message,
        &["agent api key", "未配置 agent", "模型名称不能为空"],
    ) {
        ErrorGuidance {
            code: "agent_not_configured",
            title: "Agent 模型尚未配置",
            suggestion: "请在设置中填写模型地址、模型名称和 API Key，并测试连接。",
        }
    } else if contains_any(
        &message,
        &["最大调用", "超过轮次", "调用轮次", "参数不合法", "schema"],
    ) {
        ErrorGuidance {
            code: "agent_invalid_response",
            title: "模型返回结果不符合分类规则",
            suggestion: "请重试；若持续失败，请更换支持 Tool Calling 的模型或简化候选类别描述。",
        }
    } else if contains_any(&message, &["超过 512 mib", "512 mib 本地预处理安全上限"]) {
        ErrorGuidance {
            code: "pdf_source_too_large",
            title: "PDF 超过 512 MiB 安全上限",
            suggestion: "请先在可信工具中无损拆分该 PDF，再把拆分后的文件放入监控目录。",
        }
    } else if contains_any(
        &message,
        &[
            "单页分片达到或超过 190 mb",
            "单页分片仍超过 190 mb",
            "单页分片超过安全大小",
        ],
    ) {
        ErrorGuidance {
            code: "pdf_page_too_large",
            title: "PDF 单页体积过大",
            suggestion: "当前版本不会有损压缩页面；请先手动拆解或优化该页后重试。",
        }
    } else if contains_any(&message, &["分片合并失败", "不能合并旧 mineru 分片"]) {
        ErrorGuidance {
            code: "mineru_part_merge_failed",
            title: "MinerU 分片结果合并失败",
            suggestion: "已完成分片会保留；请重试父任务以重新执行最终合并。",
        }
    } else if contains_any(
        &message,
        &["pdf 分片", "拆分失败", "无法安装隔离的 pdf 分片"],
    ) {
        ErrorGuidance {
            code: "pdf_split_failed",
            title: "PDF 本地拆分失败",
            suggestion: "请确认磁盘空间和文件权限充足；问题持续时可手动无损拆分后重试。",
        }
    } else if contains_any(&message, &["pdf 已损坏", "pdf 分片无法重新读取"]) {
        ErrorGuidance {
            code: "pdf_invalid",
            title: "PDF 已损坏或结构不受支持",
            suggestion: "请先使用 PDF 阅读器重新另存或修复文件，然后重试。",
        }
    } else if contains_any(&message, &["mineru 分片解析失败", "个 mineru 分片解析失败"])
    {
        ErrorGuidance {
            code: "mineru_part_failed",
            title: "部分 MinerU 分片失败",
            suggestion: "请在任务列表中重试失败分片；已完成分片不会重复提交。",
        }
    } else if contains_any(&message, &["encrypted", "password", "加密", "密码"]) {
        ErrorGuidance {
            code: "document_encrypted",
            title: "文档受到密码保护",
            suggestion: "请先移除文档密码或加密保护，再重新转换。",
        }
    } else if contains_any(
        &message,
        &[
            "permission denied",
            "access is denied",
            "拒绝访问",
            "权限不足",
            "无法写入",
        ],
    ) {
        ErrorGuidance {
            code: "permission_denied",
            title: "没有足够的文件访问权限",
            suggestion: "请关闭占用文件的程序，并确认当前用户可以读写监控目录和输出目录。",
        }
    } else if contains_any(
        &message,
        &[
            "being used",
            "used by another process",
            "被占用",
            "共享冲突",
        ],
    ) {
        ErrorGuidance {
            code: "file_in_use",
            title: "文件正在被其他程序占用",
            suggestion: "请关闭正在编辑该文件的程序，等待保存完成后重试。",
        }
    } else if contains_any(&message, &["not found", "找不到", "不存在", "no such file"]) {
        ErrorGuidance {
            code: "file_not_found",
            title: "文件或目录已不存在",
            suggestion: "请确认源文件和监控目录仍然存在，然后重新扫描目录。",
        }
    } else if contains_any(&message, &["不支持", "unsupported", "未知转换引擎"]) {
        ErrorGuidance {
            code: "unsupported_format",
            title: "当前转换方式不支持该文档",
            suggestion: "请检查格式开关；旧版 Office、PDF 和图片应使用 MinerU。",
        }
    } else if contains_any(
        &message,
        &["未产生 markdown", "没有产生 markdown", "内容为空", "empty"],
    ) {
        ErrorGuidance {
            code: "empty_output",
            title: "转换器没有产生 Markdown 内容",
            suggestion: "请确认源文档包含可读取内容；扫描件可改用 MinerU 后重试。",
        }
    } else if contains_any(&message, &["yaml", "frontmatter"]) {
        ErrorGuidance {
            code: "invalid_yaml",
            title: "Markdown YAML 格式无效",
            suggestion: "请修复文件开头的 YAML frontmatter，再重新分类。",
        }
    } else {
        ErrorGuidance {
            code: "unexpected_error",
            title: "任务执行失败",
            suggestion: "请查看技术详情后重试；若问题重复出现，可在帮助页复制诊断信息。",
        }
    };
    Some(guidance)
}

fn contains_any(value: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| value.contains(pattern))
}

pub async fn run_health_check(state: &AppState) -> HealthReport {
    let settings = state.settings.read().await.clone();
    let mut checks = Vec::new();

    checks.push(match state.storage.check_database() {
        Ok(()) => check_ok("database", "任务数据库", "数据库可以正常读取和写入。"),
        Err(error) => check_error(
            "database",
            "任务数据库",
            format!("数据库检查失败：{error}"),
            "请确认应用数据目录可写，并关闭可能占用数据库的程序。",
        ),
    });

    checks.push(match state.storage.check_settings_files() {
        Ok(detail) => check_ok("settings", "设置与备份", detail),
        Err(error) => check_error(
            "settings",
            "设置与备份",
            format!("设置检查失败：{error}"),
            "请先复制诊断信息并备份应用数据目录，再重新启动程序。",
        ),
    });

    if settings.profiles.is_empty() {
        checks.push(check_warning(
            "profiles",
            "监控目录",
            "尚未配置监控目录。",
            "请从帮助页开始配置监控目录和独立输出目录。",
        ));
    } else {
        for profile in &settings.profiles {
            checks.extend(check_profile(profile));
        }
        checks.extend(check_profile_overlaps(&settings.profiles));
    }

    let needs_mineru = settings.enabled_extensions.iter().any(|extension| {
        matches!(
            extension.as_str(),
            "pdf" | "doc" | "ppt" | "png" | "jpg" | "jpeg" | "webp" | "bmp"
        )
    });
    checks.push(if !needs_mineru {
        check_ok("mineru", "MinerU", "当前启用格式不需要 MinerU。")
    } else if settings.mineru_configured && AppState::read_mineru_token().is_ok() {
        check_ok("mineru", "MinerU", "Token 已保存在 Windows 凭据管理器中。")
    } else {
        check_warning(
            "mineru",
            "MinerU",
            "需要云端解析的格式已启用，但尚未保存 Token。",
            "请在设置中申请并保存 MinerU Token。",
        )
    });

    let needs_agent = settings
        .profiles
        .iter()
        .any(|profile| profile.tagging.enabled);
    checks.push(if !needs_agent {
        check_ok("agent", "Agent 分类", "当前没有目录启用 Agent 分类。")
    } else if settings.agent.configured
        && !settings.agent.model.trim().is_empty()
        && AppState::read_agent_api_key().is_ok()
    {
        check_ok(
            "agent",
            "Agent 分类",
            "模型和 API Key 已配置；本次检查未调用模型。",
        )
    } else {
        check_warning(
            "agent",
            "Agent 分类",
            "已有目录启用分类，但 Agent 模型配置不完整。",
            "请在设置中完成 Agent 配置并执行“测试连接”。",
        )
    });

    checks.push(runtime_check(
        "conversion_runtime",
        "转换后台",
        state.runtime_error(),
    ));
    checks.push(runtime_check(
        "classification_runtime",
        "分类后台",
        state.tag_runtime_error(),
    ));
    checks.push(runtime_check(
        "index_runtime",
        "索引后台",
        state.index_runtime_error(),
    ));

    let counts = health_counts(state).unwrap_or_default();
    let overall = if checks
        .iter()
        .any(|check| matches!(check.level, HealthLevel::Error))
    {
        HealthLevel::Error
    } else if checks
        .iter()
        .any(|check| matches!(check.level, HealthLevel::Warning))
    {
        HealthLevel::Warning
    } else {
        HealthLevel::Ok
    };

    HealthReport {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        checked_at: Utc::now().to_rfc3339(),
        overall,
        checks,
        counts,
    }
}

fn health_counts(state: &AppState) -> Result<HealthCounts> {
    Ok(HealthCounts {
        conversion_pending: state
            .storage
            .count_visible_tasks_with_statuses(&[JobStatus::WaitingStable, JobStatus::Queued])?,
        conversion_active: state.storage.count_visible_tasks_with_statuses(&[
            JobStatus::Converting,
            JobStatus::WaitingParts,
            JobStatus::Uploading,
            JobStatus::Processing,
            JobStatus::Downloading,
        ])?,
        conversion_waiting_mineru: state
            .storage
            .count_visible_tasks_with_statuses(&[JobStatus::WaitingMineru])?,
        conversion_failed: state
            .storage
            .count_visible_tasks_with_statuses(&[JobStatus::Failed])?,
        classification_pending: state
            .storage
            .count_tag_jobs_with_statuses(&[TagJobStatus::Queued])?,
        classification_active: state
            .storage
            .count_tag_jobs_with_statuses(&[TagJobStatus::Reading, TagJobStatus::Writing])?,
        classification_failed: state
            .storage
            .count_tag_jobs_with_statuses(&[TagJobStatus::Failed])?,
        classification_outdated: state
            .storage
            .count_tag_jobs_with_statuses(&[TagJobStatus::Outdated])?,
    })
}

pub async fn diagnostic_report(state: &AppState) -> Result<String> {
    let report = run_health_check(state).await;
    let settings = state.settings.read().await.clone();
    let mut output = String::new();
    output.push_str("CPAH Docs 诊断信息\n");
    output.push_str(&format!("版本: {}\n", report.app_version));
    output.push_str(&format!("检查时间: {}\n", report.checked_at));
    output.push_str(&format!(
        "系统: {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    output.push_str(&format!(
        "转换状态: 待执行 {} / 进行中 {} / 等待 MinerU {} / 失败 {}\n",
        report.counts.conversion_pending,
        report.counts.conversion_active,
        report.counts.conversion_waiting_mineru,
        report.counts.conversion_failed
    ));
    output.push_str(&format!(
        "分类状态: 待执行 {} / 进行中 {} / 失败 {} / 过期 {}\n",
        report.counts.classification_pending,
        report.counts.classification_active,
        report.counts.classification_failed,
        report.counts.classification_outdated
    ));
    output.push_str("\n检查项目:\n");
    for check in report.checks {
        output.push_str(&format!(
            "- [{}] {}: {}\n",
            level_text(&check.level),
            check.title,
            sanitize_text(&check.detail, &settings)
        ));
        if let Some(suggestion) = check.suggestion {
            output.push_str(&format!(
                "  建议: {}\n",
                sanitize_text(&suggestion, &settings)
            ));
        }
    }
    output.push_str("\n隐私说明: 路径已替换为目录名称；报告不包含 Token、API Key 或文档正文。\n");
    Ok(output)
}

fn check_profile(profile: &WatchProfile) -> Vec<HealthCheck> {
    vec![
        check_directory(profile, "input", Path::new(&profile.input_dir), false),
        check_directory(profile, "output", Path::new(&profile.output_dir), true),
    ]
}

fn check_directory(profile: &WatchProfile, kind: &str, path: &Path, writable: bool) -> HealthCheck {
    let id = format!("profile_{}_{}", profile.id, kind);
    let label = if kind == "input" {
        "监控目录"
    } else {
        "输出目录"
    };
    let title = format!("{} · {}", profile.name, label);
    if !path.is_dir() {
        return check_error(
            id,
            title,
            format!("{}不存在。", label),
            "请重新选择一个有效目录并保存。".to_string(),
        );
    }
    if let Err(error) = fs::read_dir(path) {
        return check_error(
            id,
            title,
            format!("无法读取目录：{error}"),
            "请检查当前 Windows 用户的目录权限。".to_string(),
        );
    }
    if writable && let Err(error) = probe_write(path) {
        return check_error(
            id,
            title,
            format!("无法写入目录：{error}"),
            "请检查目录权限、磁盘空间和安全软件拦截。".to_string(),
        );
    }
    check_ok(
        id,
        title,
        if writable {
            "目录可以正常读取和写入。"
        } else {
            "目录可以正常读取。"
        },
    )
}

fn probe_write(directory: &Path) -> Result<()> {
    let path = directory.join(format!(".cpah-health-{}.tmp", Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        file.write_all(b"CPAH Docs health probe")?;
        file.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(&path);
    result
}

fn check_profile_overlaps(profiles: &[WatchProfile]) -> Vec<HealthCheck> {
    let mut checks = Vec::new();
    for profile in profiles {
        let input = Path::new(&profile.input_dir);
        let output = Path::new(&profile.output_dir);
        if paths_overlap(input, output) {
            checks.push(check_error(
                format!("profile_overlap_{}", profile.id),
                format!("{} · 目录关系", profile.name),
                "监控目录和输出目录相同或互相包含。",
                "请改为两个互相独立的目录。",
            ));
        }
    }
    for left in 0..profiles.len() {
        for right in (left + 1)..profiles.len() {
            let a = &profiles[left];
            let b = &profiles[right];
            if paths_overlap(Path::new(&a.input_dir), Path::new(&b.input_dir))
                || paths_overlap(Path::new(&a.output_dir), Path::new(&b.output_dir))
                || paths_overlap(Path::new(&a.input_dir), Path::new(&b.output_dir))
                || paths_overlap(Path::new(&b.input_dir), Path::new(&a.output_dir))
            {
                checks.push(check_error(
                    format!("profile_cross_{}_{}", a.id, b.id),
                    "目录配置交叉",
                    format!("“{}”与“{}”的目录存在重叠。", a.name, b.name),
                    "请为每组配置使用互不包含的独立目录。",
                ));
            }
        }
    }
    checks
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn runtime_check(id: &str, title: &str, error: Option<String>) -> HealthCheck {
    match error {
        Some(error) => check_error(
            id,
            title,
            error,
            "请重新启动程序；若仍然出现，请复制诊断信息用于排查。",
        ),
        None => check_ok(id, title, "后台运行正常。"),
    }
}

fn check_ok(
    id: impl Into<String>,
    title: impl Into<String>,
    detail: impl Into<String>,
) -> HealthCheck {
    HealthCheck {
        id: id.into(),
        title: title.into(),
        level: HealthLevel::Ok,
        detail: detail.into(),
        suggestion: None,
    }
}

fn check_warning(
    id: impl Into<String>,
    title: impl Into<String>,
    detail: impl Into<String>,
    suggestion: impl Into<String>,
) -> HealthCheck {
    HealthCheck {
        id: id.into(),
        title: title.into(),
        level: HealthLevel::Warning,
        detail: detail.into(),
        suggestion: Some(suggestion.into()),
    }
}

fn check_error(
    id: impl Into<String>,
    title: impl Into<String>,
    detail: impl Into<String>,
    suggestion: impl Into<String>,
) -> HealthCheck {
    HealthCheck {
        id: id.into(),
        title: title.into(),
        level: HealthLevel::Error,
        detail: detail.into(),
        suggestion: Some(suggestion.into()),
    }
}

fn level_text(level: &HealthLevel) -> &'static str {
    match level {
        HealthLevel::Ok => "正常",
        HealthLevel::Warning => "提醒",
        HealthLevel::Error => "异常",
    }
}

fn sanitize_text(value: &str, settings: &AppSettings) -> String {
    let mut sanitized = value.to_string();
    for profile in &settings.profiles {
        for (path, kind) in [
            (&profile.input_dir, "监控目录"),
            (&profile.output_dir, "输出目录"),
        ] {
            if !path.is_empty() {
                sanitized = sanitized.replace(path, &format!("<{kind}:{}>", profile.name));
            }
        }
    }
    if let Ok(token) = AppState::read_mineru_token()
        && !token.is_empty()
    {
        sanitized = sanitized.replace(&token, "<已隐藏 MinerU Token>");
    }
    if let Ok(key) = AppState::read_agent_api_key()
        && !key.is_empty()
    {
        sanitized = sanitized.replace(&key, "<已隐藏 Agent API Key>");
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_errors() {
        assert_eq!(
            classify_error(Some("HTTP 429 rate limit")).unwrap().code,
            "service_rate_limited"
        );
        assert_eq!(
            classify_error(Some("工作簿已加密")).unwrap().code,
            "document_encrypted"
        );
        assert_eq!(
            classify_error(Some("模型未调用分类工具")).unwrap().code,
            "agent_tool_calling"
        );
        assert_eq!(
            classify_error(Some("PDF 超过 512 MiB 本地预处理安全上限"))
                .unwrap()
                .code,
            "pdf_source_too_large"
        );
        assert_eq!(
            classify_error(Some("2 个 MinerU 分片解析失败"))
                .unwrap()
                .code,
            "mineru_part_failed"
        );
        assert_eq!(
            classify_error(Some("MinerU 分片合并失败：磁盘已满"))
                .unwrap()
                .code,
            "mineru_part_merge_failed"
        );
    }

    #[test]
    fn overlap_detects_nested_paths() {
        assert!(paths_overlap(Path::new("C:/a"), Path::new("C:/a/b")));
        assert!(!paths_overlap(Path::new("C:/a"), Path::new("C:/b")));
    }
}
