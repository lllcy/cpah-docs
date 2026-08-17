use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeletePolicy {
    #[default]
    Trash,
    Keep,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchProfile {
    pub id: String,
    pub name: String,
    pub input_dir: String,
    pub output_dir: String,
    pub enabled: bool,
    #[serde(default)]
    pub delete_policy: DeletePolicy,
    #[serde(default)]
    pub tagging: TaggingConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TagSelectionMode {
    #[default]
    Single,
    Multiple,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryLabel {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaggingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub selection_mode: TagSelectionMode,
    #[serde(default)]
    pub labels: Vec<CategoryLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettings {
    #[serde(default = "default_agent_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub configured: bool,
    #[serde(default = "default_tag_concurrency")]
    pub concurrency: u8,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            base_url: default_agent_base_url(),
            model: String::new(),
            configured: false,
            concurrency: default_tag_concurrency(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub profiles: Vec<WatchProfile>,
    #[serde(default)]
    pub monitoring_paused: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub classification_paused: bool,
    #[serde(default = "default_mineru_base_url")]
    pub mineru_base_url: String,
    #[serde(default)]
    pub mineru_configured: bool,
    #[serde(default = "default_enabled_extensions")]
    pub enabled_extensions: Vec<String>,
    #[serde(default)]
    pub split_enabled: bool,
    #[serde(default = "default_split_max_pages")]
    pub split_max_pages: u32,
    #[serde(default = "default_split_overlap_pages")]
    pub split_overlap_pages: u32,
    #[serde(default)]
    pub split_temp_dir: Option<String>,
    #[serde(default)]
    pub split_keep_temp: bool,
    #[serde(default)]
    pub agent: AgentSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            monitoring_paused: false,
            // A brand-new user must explicitly start conversion after reviewing the
            // configured input/output pair. Persisted users keep their saved value.
            paused: true,
            classification_paused: true,
            mineru_base_url: default_mineru_base_url(),
            mineru_configured: false,
            enabled_extensions: default_enabled_extensions(),
            split_enabled: true,
            split_max_pages: default_split_max_pages(),
            split_overlap_pages: default_split_overlap_pages(),
            split_temp_dir: None,
            split_keep_temp: false,
            agent: AgentSettings::default(),
        }
    }
}

pub fn default_enabled_extensions() -> Vec<String> {
    [
        "md", "docx", "xlsx", "xls", "pptx", "html", "htm", "csv", "txt", "pdf", "doc", "ppt",
        "png", "jpg", "jpeg", "webp", "bmp",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn default_mineru_base_url() -> String {
    "https://mineru.net/api/v4".to_string()
}

fn default_split_max_pages() -> u32 {
    200
}

fn default_split_overlap_pages() -> u32 {
    5
}

fn default_agent_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_tag_concurrency() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversionEngine {
    Anytomd,
    Mineru,
}

impl ConversionEngine {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Anytomd => "anytomd",
            Self::Mineru => "mineru",
        }
    }
}

impl TryFrom<&str> for ConversionEngine {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "anytomd" => Ok(Self::Anytomd),
            "mineru" => Ok(Self::Mineru),
            _ => anyhow::bail!("未知转换引擎：{value}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    WaitingStable,
    Queued,
    Converting,
    WaitingMineru,
    Uploading,
    Processing,
    Downloading,
    Completed,
    Failed,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WaitingStable => "waiting_stable",
            Self::Queued => "queued",
            Self::Converting => "converting",
            Self::WaitingMineru => "waiting_mineru",
            Self::Uploading => "uploading",
            Self::Processing => "processing",
            Self::Downloading => "downloading",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl TryFrom<&str> for JobStatus {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "waiting_stable" => Ok(Self::WaitingStable),
            "queued" => Ok(Self::Queued),
            "converting" => Ok(Self::Converting),
            "waiting_mineru" => Ok(Self::WaitingMineru),
            "uploading" => Ok(Self::Uploading),
            "processing" => Ok(Self::Processing),
            "downloading" => Ok(Self::Downloading),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => anyhow::bail!("未知任务状态：{value}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub profile_id: String,
    pub source_path: String,
    pub relative_path: String,
    pub source_hash: Option<String>,
    pub source_size: Option<i64>,
    pub source_modified_ms: Option<i64>,
    pub engine: ConversionEngine,
    pub status: JobStatus,
    pub output_path: Option<String>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub error_title: Option<String>,
    pub error_suggestion: Option<String>,
    pub mineru_batch_id: Option<String>,
    pub mineru_data_id: Option<String>,
    pub mineru_state: Option<String>,
    pub mineru_extracted_pages: Option<i64>,
    pub mineru_total_pages: Option<i64>,
    pub mineru_started_at: Option<String>,
    pub updated_at: String,
    pub tag_job_id: Option<String>,
    pub tag_status: Option<TagJobStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TagJobStatus {
    Queued,
    Reading,
    Writing,
    Completed,
    Failed,
    Outdated,
    Cancelled,
}

impl TagJobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Reading => "reading",
            Self::Writing => "writing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Outdated => "outdated",
            Self::Cancelled => "cancelled",
        }
    }
}

impl TryFrom<&str> for TagJobStatus {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "queued" => Ok(Self::Queued),
            "reading" => Ok(Self::Reading),
            "writing" => Ok(Self::Writing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "outdated" => Ok(Self::Outdated),
            "cancelled" => Ok(Self::Cancelled),
            _ => anyhow::bail!("未知分类任务状态：{value}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagJobRecord {
    pub id: String,
    pub profile_id: String,
    pub markdown_path: String,
    pub relative_path: String,
    pub status: TagJobStatus,
    pub content_hash: Option<String>,
    pub schema_hash: String,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub error_title: Option<String>,
    pub error_suggestion: Option<String>,
    pub read_bytes: i64,
    pub total_bytes: i64,
    pub api_calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaggingImpact {
    pub discovered: usize,
    pub new_files: usize,
    pub affected: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub settings: AppSettings,
    pub tasks: Vec<TaskRecord>,
    pub tag_jobs: Vec<TagJobRecord>,
    pub task_total: usize,
    pub tag_job_total: usize,
    pub runtime_error: Option<String>,
    pub tag_runtime_error: Option<String>,
    pub index_runtime_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthLevel {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    pub id: String,
    pub title: String,
    pub level: HealthLevel,
    pub detail: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCounts {
    pub conversion_pending: usize,
    pub conversion_active: usize,
    pub conversion_waiting_mineru: usize,
    pub conversion_failed: usize,
    pub classification_pending: usize,
    pub classification_active: usize,
    pub classification_failed: usize,
    pub classification_outdated: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub app_version: String,
    pub checked_at: String,
    pub overall: HealthLevel,
    pub checks: Vec<HealthCheck>,
    pub counts: HealthCounts,
}
