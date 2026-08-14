use crate::models::{
    AppSettings, ConversionEngine, JobStatus, TagJobRecord, TagJobStatus, TaskRecord, WatchProfile,
};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone)]
pub struct Storage {
    db_path: PathBuf,
    settings_path: PathBuf,
    settings_backup_path: PathBuf,
}

impl Storage {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("无法创建应用数据目录：{}", data_dir.display()))?;
        let storage = Self {
            db_path: data_dir.join("converter.db"),
            settings_path: data_dir.join("settings.json"),
            settings_backup_path: data_dir.join("settings.backup.json"),
        };
        storage.initialize_database()?;
        Ok(storage)
    }

    fn open(&self) -> Result<Connection> {
        let connection = Connection::open(&self.db_path)
            .with_context(|| format!("无法打开任务数据库：{}", self.db_path.display()))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(connection)
    }

    pub fn check_database(&self) -> Result<()> {
        let connection = self.open()?;
        connection
            .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .context("数据库完整性检查失败")
            .and_then(|result| {
                if result.eq_ignore_ascii_case("ok") {
                    Ok(())
                } else {
                    anyhow::bail!("数据库完整性检查返回：{result}")
                }
            })?;
        connection
            .execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
            .context("数据库写入检查失败")?;
        Ok(())
    }

    pub fn check_settings_files(&self) -> Result<String> {
        if self.settings_path.exists() {
            read_settings_file(&self.settings_path).context("主设置文件无效")?;
        }
        if self.settings_backup_path.exists() {
            read_settings_file(&self.settings_backup_path).context("设置备份无效")?;
        }
        Ok(match (
            self.settings_path.exists(),
            self.settings_backup_path.exists(),
        ) {
            (true, true) => "主设置和上一份有效备份均可读取。",
            (true, false) => "主设置可读取；首次再次保存后会生成备份。",
            (false, _) => "尚未生成设置文件，将在首次保存时创建。",
        }
        .to_string())
    }

    fn initialize_database(&self) -> Result<()> {
        let connection = self.open()?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS tasks (
               id TEXT PRIMARY KEY,
               profile_id TEXT NOT NULL,
               source_path TEXT NOT NULL UNIQUE,
               relative_path TEXT NOT NULL,
               source_hash TEXT,
               source_size INTEGER,
               source_modified_ms INTEGER,
               engine TEXT NOT NULL,
               status TEXT NOT NULL,
               output_path TEXT,
               error TEXT,
               mineru_batch_id TEXT,
               mineru_data_id TEXT,
               mineru_state TEXT,
               mineru_extracted_pages INTEGER,
               mineru_total_pages INTEGER,
               mineru_started_at TEXT,
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_tasks_profile ON tasks(profile_id);
             CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
             CREATE TABLE IF NOT EXISTS tag_jobs (
               id TEXT PRIMARY KEY,
               profile_id TEXT NOT NULL,
               markdown_path TEXT NOT NULL UNIQUE,
               relative_path TEXT NOT NULL,
               status TEXT NOT NULL,
               content_hash TEXT,
               schema_hash TEXT NOT NULL,
               result_json TEXT,
               error TEXT,
               read_bytes INTEGER NOT NULL DEFAULT 0,
               total_bytes INTEGER NOT NULL DEFAULT 0,
               api_calls INTEGER NOT NULL DEFAULT 0,
               input_tokens INTEGER NOT NULL DEFAULT 0,
               output_tokens INTEGER NOT NULL DEFAULT 0,
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_tag_jobs_profile ON tag_jobs(profile_id);
             CREATE INDEX IF NOT EXISTS idx_tag_jobs_status ON tag_jobs(status);",
        )?;
        for (name, declaration) in [
            ("source_size", "INTEGER"),
            ("source_modified_ms", "INTEGER"),
            ("mineru_state", "TEXT"),
            ("mineru_extracted_pages", "INTEGER"),
            ("mineru_total_pages", "INTEGER"),
            ("mineru_started_at", "TEXT"),
        ] {
            ensure_column(&connection, "tasks", name, declaration)?;
        }
        Ok(())
    }

    pub fn load_settings(&self) -> Result<AppSettings> {
        if self.settings_path.exists() {
            match read_settings_file(&self.settings_path) {
                Ok(settings) => return Ok(settings),
                Err(_) if self.settings_backup_path.exists() => {
                    let bytes = fs::read(&self.settings_backup_path)
                        .context("主设置损坏，且无法读取设置备份")?;
                    let settings =
                        serde_json::from_slice(&bytes).context("主设置和设置备份均无效")?;
                    crate::atomic_file::write_atomic(&self.settings_path, &bytes)
                        .context("从备份恢复设置失败")?;
                    return Ok(settings);
                }
                Err(error) => return Err(error.context("设置文件格式无效且没有可用备份")),
            }
        }
        if self.settings_backup_path.exists() {
            let bytes = fs::read(&self.settings_backup_path).context("无法读取设置备份")?;
            let settings = serde_json::from_slice(&bytes).context("设置备份格式无效")?;
            crate::atomic_file::write_atomic(&self.settings_path, &bytes)
                .context("从备份恢复设置失败")?;
            return Ok(settings);
        }
        Ok(AppSettings::default())
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let content = serde_json::to_vec_pretty(settings)?;
        if self.settings_path.exists() {
            let current = fs::read(&self.settings_path)?;
            if serde_json::from_slice::<AppSettings>(&current).is_ok() {
                crate::atomic_file::write_atomic(&self.settings_backup_path, &current)
                    .context("无法更新设置备份")?;
            }
        }
        crate::atomic_file::write_atomic(&self.settings_path, &content).context("无法原子保存设置")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_task(
        &self,
        profile: &WatchProfile,
        source_path: &Path,
        relative_path: &Path,
        source_hash: &str,
        source_size: u64,
        source_modified_ms: i64,
        engine: ConversionEngine,
        output_path: &Path,
        force: bool,
    ) -> Result<Option<TaskRecord>> {
        let source = source_path.to_string_lossy().to_string();
        let output = output_path.to_string_lossy().to_string();
        if !force && let Some(existing) = self.find_by_source(&source)? {
            if existing.source_hash.as_deref() == Some(source_hash)
                && existing.status == JobStatus::Completed
                && existing.output_path.as_deref() == Some(output.as_str())
                && Path::new(&output).exists()
            {
                self.update_source_metadata(&existing.id, source_size as i64, source_modified_ms)?;
                return Ok(None);
            }
            if existing.source_hash.as_deref() == Some(source_hash)
                && existing.output_path.as_deref() == Some(output.as_str())
                && matches!(
                    existing.status,
                    JobStatus::Failed | JobStatus::WaitingMineru
                )
            {
                self.update_source_metadata(&existing.id, source_size as i64, source_modified_ms)?;
                return Ok(None);
            }
        }

        let now = Utc::now().to_rfc3339();
        let existing_id = self.find_by_source(&source)?.map(|task| task.id);
        let id = existing_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO tasks (
               id, profile_id, source_path, relative_path, source_hash,
               source_size, source_modified_ms, engine, status, output_path,
               error, mineru_batch_id, mineru_data_id, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, NULL, NULL, ?11, ?11)
             ON CONFLICT(source_path) DO UPDATE SET
               profile_id=excluded.profile_id,
               relative_path=excluded.relative_path,
               source_hash=excluded.source_hash,
               source_size=excluded.source_size,
               source_modified_ms=excluded.source_modified_ms,
               engine=excluded.engine,
               status=excluded.status,
               output_path=excluded.output_path,
               error=NULL,
               mineru_batch_id=NULL,
               mineru_data_id=NULL,
               mineru_state=NULL,
               mineru_extracted_pages=NULL,
               mineru_total_pages=NULL,
               mineru_started_at=NULL,
               updated_at=excluded.updated_at",
            params![
                id,
                profile.id,
                source,
                relative_path.to_string_lossy(),
                source_hash,
                source_size as i64,
                source_modified_ms,
                engine.as_str(),
                JobStatus::Queued.as_str(),
                output,
                now,
            ],
        )?;
        self.get_task(&id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn queue_task(
        &self,
        profile: &WatchProfile,
        source_path: &Path,
        relative_path: &Path,
        source_size: u64,
        source_modified_ms: i64,
        engine: ConversionEngine,
        output_path: &Path,
        force: bool,
    ) -> Result<Option<TaskRecord>> {
        let source = source_path.to_string_lossy().to_string();
        let output = output_path.to_string_lossy().to_string();
        let existing = self.find_by_source(&source)?;
        if !force
            && let Some(existing) = existing.as_ref()
            && existing.source_size == Some(source_size as i64)
            && existing.source_modified_ms == Some(source_modified_ms)
            && existing.output_path.as_deref() == Some(output.as_str())
        {
            if existing.status == JobStatus::Completed && Path::new(&output).exists() {
                return Ok(None);
            }
            if matches!(
                existing.status,
                JobStatus::Failed | JobStatus::WaitingMineru
            ) {
                return Ok(None);
            }
        }
        if !force
            && let Some(existing) = existing.as_ref()
            && (existing.source_size.is_none() || existing.source_modified_ms.is_none())
            && existing.output_path.as_deref() == Some(output.as_str())
            && existing.status == JobStatus::Completed
            && Path::new(&output).exists()
            && output_modified_ms(Path::new(&output))
                .is_some_and(|output_modified| output_modified >= source_modified_ms)
        {
            // The generated result is at least as new as the source, so the legacy row can
            // safely adopt the current metadata without putting an old document back in line.
            self.update_source_metadata(&existing.id, source_size as i64, source_modified_ms)?;
            return Ok(None);
        }
        if !force
            && let Some(existing) = existing.as_ref()
            && (existing.source_size.is_none() || existing.source_modified_ms.is_none())
            && existing.output_path.as_deref() == Some(output.as_str())
            && ((existing.status == JobStatus::Completed && Path::new(&output).exists())
                || matches!(
                    existing.status,
                    JobStatus::Failed | JobStatus::WaitingMineru
                ))
        {
            // Legacy databases do not have size/mtime. Preserve the visible status while
            // the worker verifies the existing hash, then backfill the metadata above.
            return Ok(Some(existing.clone()));
        }

        let now = Utc::now().to_rfc3339();
        let id = existing
            .map(|task| task.id)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO tasks (
               id, profile_id, source_path, relative_path, source_hash,
               source_size, source_modified_ms, engine, status, output_path,
               error, mineru_batch_id, mineru_data_id, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, NULL, NULL, NULL, ?10, ?10)
             ON CONFLICT(source_path) DO UPDATE SET
               profile_id=excluded.profile_id,
               relative_path=excluded.relative_path,
               source_size=excluded.source_size,
               source_modified_ms=excluded.source_modified_ms,
               engine=excluded.engine,
               status=excluded.status,
               output_path=excluded.output_path,
               error=NULL,
               mineru_batch_id=NULL,
               mineru_data_id=NULL,
               mineru_state=NULL,
               mineru_extracted_pages=NULL,
               mineru_total_pages=NULL,
               mineru_started_at=NULL,
               updated_at=excluded.updated_at",
            params![
                id,
                profile.id,
                source,
                relative_path.to_string_lossy(),
                source_size as i64,
                source_modified_ms,
                engine.as_str(),
                JobStatus::Queued.as_str(),
                output,
                now,
            ],
        )?;
        self.get_task(&id)
    }

    pub fn set_status(&self, id: &str, status: JobStatus, error: Option<&str>) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "UPDATE tasks SET status=?2, error=?3, updated_at=?4 WHERE id=?1",
            params![id, status.as_str(), error, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    fn update_source_metadata(
        &self,
        id: &str,
        source_size: i64,
        source_modified_ms: i64,
    ) -> Result<()> {
        self.open()?.execute(
            "UPDATE tasks SET source_size=?2, source_modified_ms=?3 WHERE id=?1",
            params![id, source_size, source_modified_ms],
        )?;
        Ok(())
    }

    pub fn set_mineru_submission(
        &self,
        id: &str,
        batch_id: &str,
        data_id: &str,
        status: JobStatus,
    ) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "UPDATE tasks SET mineru_batch_id=?2, mineru_data_id=?3, status=?4,
                    mineru_state=NULL, mineru_extracted_pages=NULL,
                    mineru_total_pages=NULL, mineru_started_at=NULL, updated_at=?5
             WHERE id=?1",
            params![
                id,
                batch_id,
                data_id,
                status.as_str(),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn set_mineru_progress(
        &self,
        id: &str,
        state: Option<&str>,
        extracted_pages: Option<i64>,
        total_pages: Option<i64>,
        started_at: Option<&str>,
    ) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "UPDATE tasks SET mineru_state=?2, mineru_extracted_pages=?3,
                    mineru_total_pages=?4, mineru_started_at=?5, updated_at=?6
             WHERE id=?1",
            params![
                id,
                state,
                extracted_pages,
                total_pages,
                started_at,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_task(&self, id: &str) -> Result<Option<TaskRecord>> {
        let connection = self.open()?;
        connection
            .query_row(
                "SELECT id, profile_id, source_path, relative_path, source_hash,
                        source_size, source_modified_ms, engine,
                        status, output_path, error, mineru_batch_id, mineru_data_id,
                        mineru_state, mineru_extracted_pages, mineru_total_pages,
                        mineru_started_at, updated_at
                 FROM tasks WHERE id=?1",
                [id],
                map_task,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn find_by_source(&self, source_path: &str) -> Result<Option<TaskRecord>> {
        let connection = self.open()?;
        connection
            .query_row(
                "SELECT id, profile_id, source_path, relative_path, source_hash,
                        source_size, source_modified_ms, engine,
                        status, output_path, error, mineru_batch_id, mineru_data_id,
                        mineru_state, mineru_extracted_pages, mineru_total_pages,
                        mineru_started_at, updated_at
                 FROM tasks WHERE source_path=?1",
                [source_path],
                map_task,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_tasks(&self, limit: usize) -> Result<Vec<TaskRecord>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT id, profile_id, source_path, relative_path, source_hash,
                    source_size, source_modified_ms, engine,
                    status, output_path, error, mineru_batch_id, mineru_data_id,
                    mineru_state, mineru_extracted_pages, mineru_total_pages,
                    mineru_started_at, updated_at
             FROM tasks ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit as i64], map_task)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn list_profile_tasks(&self, profile_id: &str) -> Result<Vec<TaskRecord>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT id, profile_id, source_path, relative_path, source_hash,
                    source_size, source_modified_ms, engine,
                    status, output_path, error, mineru_batch_id, mineru_data_id,
                    mineru_state, mineru_extracted_pages, mineru_total_pages,
                    mineru_started_at, updated_at
             FROM tasks WHERE profile_id=?1",
        )?;
        let rows = statement.query_map([profile_id], map_task)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn list_tasks_with_statuses(&self, statuses: &[JobStatus]) -> Result<Vec<TaskRecord>> {
        if statuses.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", statuses.len())
            .collect::<Vec<_>>()
            .join(",");
        let query = format!(
            "SELECT id, profile_id, source_path, relative_path, source_hash,
                    source_size, source_modified_ms, engine,
                    status, output_path, error, mineru_batch_id, mineru_data_id,
                    mineru_state, mineru_extracted_pages, mineru_total_pages,
                    mineru_started_at, updated_at
             FROM tasks WHERE status IN ({placeholders}) ORDER BY updated_at ASC"
        );
        let connection = self.open()?;
        let mut statement = connection.prepare(&query)?;
        let rows = statement.query_map(
            params_from_iter(statuses.iter().map(JobStatus::as_str)),
            map_task,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn task_count(&self) -> Result<usize> {
        self.open()?
            .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, i64>(0))
            .map(|count| count.max(0) as usize)
            .map_err(Into::into)
    }

    pub fn count_tasks_with_statuses(&self, statuses: &[JobStatus]) -> Result<usize> {
        count_statuses(
            &self.open()?,
            "tasks",
            statuses.iter().map(JobStatus::as_str),
        )
    }

    pub fn delete_profile_records(&self, profile_ids: &[String]) -> Result<()> {
        if profile_ids.is_empty() {
            return Ok(());
        }
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        for profile_id in profile_ids {
            transaction.execute("DELETE FROM tasks WHERE profile_id=?1", [profile_id])?;
            transaction.execute("DELETE FROM tag_jobs WHERE profile_id=?1", [profile_id])?;
        }
        transaction.commit().map_err(Into::into)
    }

    pub fn delete_task(&self, id: &str) -> Result<()> {
        self.open()?
            .execute("DELETE FROM tasks WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn delete_disabled_waiting_tasks(&self, enabled_extensions: &[String]) -> Result<()> {
        let enabled = enabled_extensions
            .iter()
            .map(|extension| extension.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        for task in self.list_tasks_with_statuses(&[
            JobStatus::WaitingStable,
            JobStatus::Queued,
            JobStatus::WaitingMineru,
        ])? {
            let extension = Path::new(&task.source_path)
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase);
            if extension.is_none_or(|extension| !enabled.contains(&extension)) {
                self.delete_task(&task.id)?;
            }
        }
        Ok(())
    }

    pub fn get_tag_job(&self, id: &str) -> Result<Option<TagJobRecord>> {
        self.open()?
            .query_row(
                "SELECT id, profile_id, markdown_path, relative_path, status,
                        content_hash, schema_hash, result_json, error, read_bytes,
                        total_bytes, api_calls, input_tokens, output_tokens, updated_at
                 FROM tag_jobs WHERE id=?1",
                [id],
                map_tag_job,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn count_tag_jobs_with_statuses(&self, statuses: &[TagJobStatus]) -> Result<usize> {
        count_statuses(
            &self.open()?,
            "tag_jobs",
            statuses.iter().map(TagJobStatus::as_str),
        )
    }

    pub fn find_tag_job_by_path(&self, markdown_path: &Path) -> Result<Option<TagJobRecord>> {
        self.open()?
            .query_row(
                "SELECT id, profile_id, markdown_path, relative_path, status,
                        content_hash, schema_hash, result_json, error, read_bytes,
                        total_bytes, api_calls, input_tokens, output_tokens, updated_at
                 FROM tag_jobs WHERE markdown_path=?1",
                [markdown_path.to_string_lossy().as_ref()],
                map_tag_job,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_tag_jobs(&self, limit: usize) -> Result<Vec<TagJobRecord>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT id, profile_id, markdown_path, relative_path, status,
                    content_hash, schema_hash, result_json, error, read_bytes,
                    total_bytes, api_calls, input_tokens, output_tokens, updated_at
             FROM tag_jobs ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit as i64], map_tag_job)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn list_tag_jobs_with_statuses(
        &self,
        statuses: &[TagJobStatus],
    ) -> Result<Vec<TagJobRecord>> {
        if statuses.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", statuses.len())
            .collect::<Vec<_>>()
            .join(",");
        let query = format!(
            "SELECT id, profile_id, markdown_path, relative_path, status,
                    content_hash, schema_hash, result_json, error, read_bytes,
                    total_bytes, api_calls, input_tokens, output_tokens, updated_at
             FROM tag_jobs WHERE status IN ({placeholders}) ORDER BY updated_at ASC"
        );
        let connection = self.open()?;
        let mut statement = connection.prepare(&query)?;
        let rows = statement.query_map(
            params_from_iter(statuses.iter().map(TagJobStatus::as_str)),
            map_tag_job,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn tag_job_count(&self) -> Result<usize> {
        self.open()?
            .query_row("SELECT COUNT(*) FROM tag_jobs", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| count.max(0) as usize)
            .map_err(Into::into)
    }

    pub fn put_tag_job(
        &self,
        profile_id: &str,
        markdown_path: &Path,
        relative_path: &Path,
        schema_hash: &str,
        status: TagJobStatus,
        replace_existing: bool,
    ) -> Result<TagJobRecord> {
        let path = markdown_path.to_string_lossy().to_string();
        if let Some(existing) = self.find_tag_job_by_path(markdown_path)?
            && !replace_existing
        {
            return Ok(existing);
        }
        let now = Utc::now().to_rfc3339();
        let id = self
            .find_tag_job_by_path(markdown_path)?
            .map(|job| job.id)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        self.open()?.execute(
            "INSERT INTO tag_jobs (
               id, profile_id, markdown_path, relative_path, status, schema_hash,
               created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(markdown_path) DO UPDATE SET
               profile_id=excluded.profile_id,
               relative_path=excluded.relative_path,
               status=excluded.status,
               schema_hash=excluded.schema_hash,
               error=NULL,
               read_bytes=0,
               total_bytes=0,
               api_calls=0,
               input_tokens=0,
               output_tokens=0,
               updated_at=excluded.updated_at",
            params![
                id,
                profile_id,
                path,
                relative_path.to_string_lossy(),
                status.as_str(),
                schema_hash,
                now,
            ],
        )?;
        self.find_tag_job_by_path(markdown_path)?
            .context("分类任务写入后未找到")
    }

    pub fn set_tag_job_status(
        &self,
        id: &str,
        status: TagJobStatus,
        error: Option<&str>,
    ) -> Result<()> {
        self.open()?.execute(
            "UPDATE tag_jobs SET status=?2, error=?3, updated_at=?4 WHERE id=?1",
            params![id, status.as_str(), error, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_tag_job_usage(
        &self,
        id: &str,
        read_bytes: i64,
        total_bytes: i64,
        api_calls: i64,
        input_tokens: i64,
        output_tokens: i64,
    ) -> Result<()> {
        self.open()?.execute(
            "UPDATE tag_jobs SET read_bytes=?2, total_bytes=?3, api_calls=?4,
                    input_tokens=?5, output_tokens=?6, updated_at=?7 WHERE id=?1",
            params![
                id,
                read_bytes,
                total_bytes,
                api_calls,
                input_tokens,
                output_tokens,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn complete_tag_job(
        &self,
        id: &str,
        content_hash: &str,
        result_json: &str,
        read_bytes: i64,
        total_bytes: i64,
        api_calls: i64,
        input_tokens: i64,
        output_tokens: i64,
    ) -> Result<()> {
        self.open()?.execute(
            "UPDATE tag_jobs SET status='completed', content_hash=?2, result_json=?3,
                    error=NULL, read_bytes=?4, total_bytes=?5, api_calls=?6,
                    input_tokens=?7, output_tokens=?8, updated_at=?9 WHERE id=?1",
            params![
                id,
                content_hash,
                result_json,
                read_bytes,
                total_bytes,
                api_calls,
                input_tokens,
                output_tokens,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn mark_profile_tag_jobs_outdated(
        &self,
        profile_id: &str,
        schema_hash: &str,
    ) -> Result<usize> {
        Ok(self.open()?.execute(
            "UPDATE tag_jobs SET status='outdated', schema_hash=?2, updated_at=?3
             WHERE profile_id=?1 AND status NOT IN ('reading', 'writing')",
            params![profile_id, schema_hash, Utc::now().to_rfc3339()],
        )?)
    }

    pub fn cancel_profile_pending_tag_jobs(&self, profile_id: &str) -> Result<usize> {
        Ok(self.open()?.execute(
            "UPDATE tag_jobs SET status='cancelled', updated_at=?2
             WHERE profile_id=?1 AND status IN ('queued', 'outdated')",
            params![profile_id, Utc::now().to_rfc3339()],
        )?)
    }

    pub fn requeue_interrupted_tag_jobs(&self) -> Result<usize> {
        Ok(self.open()?.execute(
            "UPDATE tag_jobs SET status='queued', error='程序上次在任务执行中退出，已重新排队', updated_at=?1
             WHERE status IN ('reading', 'writing')",
            [Utc::now().to_rfc3339()],
        )?)
    }
}

fn read_settings_file(path: &Path) -> Result<AppSettings> {
    let bytes = fs::read(path).with_context(|| format!("无法读取设置：{}", path.display()))?;
    serde_json::from_slice(&bytes).context("设置文件格式无效")
}

fn map_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRecord> {
    let engine: String = row.get(7)?;
    let status: String = row.get(8)?;
    let error: Option<String> = row.get(10)?;
    let guidance = crate::diagnostics::classify_error(error.as_deref());
    Ok(TaskRecord {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        source_path: row.get(2)?,
        relative_path: row.get(3)?,
        source_hash: row.get(4)?,
        source_size: row.get(5)?,
        source_modified_ms: row.get(6)?,
        engine: ConversionEngine::try_from(engine.as_str()).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, error.into())
        })?,
        status: JobStatus::try_from(status.as_str()).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, error.into())
        })?,
        output_path: row.get(9)?,
        error,
        error_code: guidance.as_ref().map(|value| value.code.to_string()),
        error_title: guidance.as_ref().map(|value| value.title.to_string()),
        error_suggestion: guidance.as_ref().map(|value| value.suggestion.to_string()),
        mineru_batch_id: row.get(11)?,
        mineru_data_id: row.get(12)?,
        mineru_state: row.get(13)?,
        mineru_extracted_pages: row.get(14)?,
        mineru_total_pages: row.get(15)?,
        mineru_started_at: row.get(16)?,
        updated_at: row.get(17)?,
        tag_job_id: None,
        tag_status: None,
    })
}

fn map_tag_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<TagJobRecord> {
    let status: String = row.get(4)?;
    let error: Option<String> = row.get(8)?;
    let guidance = crate::diagnostics::classify_error(error.as_deref());
    Ok(TagJobRecord {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        markdown_path: row.get(2)?,
        relative_path: row.get(3)?,
        status: TagJobStatus::try_from(status.as_str()).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, error.into())
        })?,
        content_hash: row.get(5)?,
        schema_hash: row.get(6)?,
        result_json: row.get(7)?,
        error,
        error_code: guidance.as_ref().map(|value| value.code.to_string()),
        error_title: guidance.as_ref().map(|value| value.title.to_string()),
        error_suggestion: guidance.as_ref().map(|value| value.suggestion.to_string()),
        read_bytes: row.get(9)?,
        total_bytes: row.get(10)?,
        api_calls: row.get(11)?,
        input_tokens: row.get(12)?,
        output_tokens: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn count_statuses<'a>(
    connection: &Connection,
    table: &str,
    statuses: impl Iterator<Item = &'a str>,
) -> Result<usize> {
    let statuses = statuses.collect::<Vec<_>>();
    if statuses.is_empty() {
        return Ok(0);
    }
    let placeholders = std::iter::repeat_n("?", statuses.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!("SELECT COUNT(*) FROM {table} WHERE status IN ({placeholders})");
    connection
        .query_row(&query, params_from_iter(statuses), |row| {
            row.get::<_, i64>(0)
        })
        .map(|count| count.max(0) as usize)
        .map_err(Into::into)
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    declaration: &str,
) -> Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }
    connection.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {declaration}"),
        [],
    )?;
    Ok(())
}

fn output_modified_ms(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_preserves_category_rules() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = Storage::new(temporary.path().join("data")).unwrap();
        let mut settings = AppSettings {
            classification_paused: true,
            ..Default::default()
        };
        settings.profiles.push(WatchProfile {
            id: "wiki".into(),
            name: "wiki".into(),
            input_dir: "input".into(),
            output_dir: "output".into(),
            enabled: true,
            delete_policy: Default::default(),
            tagging: crate::models::TaggingConfig {
                enabled: true,
                selection_mode: crate::models::TagSelectionMode::Multiple,
                labels: vec![crate::models::CategoryLabel {
                    id: "training".into(),
                    name: "培训材料".into(),
                    description: "课程和讲义".into(),
                }],
            },
        });

        storage.save_settings(&settings).unwrap();
        let loaded = storage.load_settings().unwrap();
        assert_eq!(loaded.profiles[0].tagging, settings.profiles[0].tagging);
        assert!(loaded.classification_paused);
    }

    #[test]
    fn old_settings_default_to_classification_running() {
        let temporary = tempfile::tempdir().unwrap();
        let data_dir = temporary.path().join("data");
        let storage = Storage::new(data_dir.clone()).unwrap();
        std::fs::write(
            data_dir.join("settings.json"),
            r#"{"profiles":[],"paused":false,"enabledExtensions":[]}"#,
        )
        .unwrap();

        let loaded = storage.load_settings().unwrap();
        assert!(!loaded.classification_paused);
    }

    #[test]
    fn corrupt_primary_settings_are_restored_from_last_valid_backup() {
        let temporary = tempfile::tempdir().unwrap();
        let data_dir = temporary.path().join("data");
        let storage = Storage::new(data_dir.clone()).unwrap();
        let mut first = AppSettings {
            paused: true,
            classification_paused: false,
            ..Default::default()
        };
        storage.save_settings(&first).unwrap();
        first.classification_paused = true;
        storage.save_settings(&first).unwrap();
        std::fs::write(data_dir.join("settings.json"), b"{broken").unwrap();

        let recovered = storage.load_settings().unwrap();

        assert!(recovered.paused);
        assert!(!recovered.classification_paused);
        assert!(read_settings_file(&data_dir.join("settings.json")).is_ok());
    }

    #[test]
    fn migrates_task_metadata_and_progress_columns_into_an_existing_database() {
        let temporary = tempfile::tempdir().unwrap();
        let database = temporary.path().join("converter.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE tasks (
                   id TEXT PRIMARY KEY,
                   profile_id TEXT NOT NULL,
                   source_path TEXT NOT NULL UNIQUE,
                   relative_path TEXT NOT NULL,
                   source_hash TEXT,
                   engine TEXT NOT NULL,
                   status TEXT NOT NULL,
                   output_path TEXT,
                   error TEXT,
                   mineru_batch_id TEXT,
                   mineru_data_id TEXT,
                   created_at TEXT NOT NULL,
                   updated_at TEXT NOT NULL
                 );",
            )
            .unwrap();
        drop(connection);

        let storage = Storage::new(temporary.path().to_path_buf()).unwrap();
        let connection = storage.open().unwrap();
        let mut statement = connection.prepare("PRAGMA table_info(tasks)").unwrap();
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();

        for expected in [
            "source_size",
            "source_modified_ms",
            "mineru_state",
            "mineru_extracted_pages",
            "mineru_total_pages",
            "mineru_started_at",
        ] {
            assert!(columns.iter().any(|column| column == expected));
        }
    }

    #[test]
    fn queues_before_conversion_and_skips_an_unchanged_completed_file() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        let source = input.join("report.docx");
        let result = output.join("report.md");
        fs::write(&source, b"test").unwrap();
        let profile = WatchProfile {
            id: "profile".to_string(),
            name: "profile".to_string(),
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            enabled: true,
            delete_policy: Default::default(),
            tagging: Default::default(),
        };
        let storage = Storage::new(temporary.path().join("data")).unwrap();

        let task = storage
            .queue_task(
                &profile,
                &source,
                Path::new("report.docx"),
                4,
                123,
                ConversionEngine::Anytomd,
                &result,
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(task.status, JobStatus::Queued);
        assert_eq!(task.source_size, Some(4));
        assert_eq!(task.source_modified_ms, Some(123));

        fs::write(&result, b"done").unwrap();
        storage
            .set_status(&task.id, JobStatus::Completed, None)
            .unwrap();
        let unchanged = storage
            .queue_task(
                &profile,
                &source,
                Path::new("report.docx"),
                4,
                123,
                ConversionEngine::Anytomd,
                &result,
                false,
            )
            .unwrap();
        assert!(unchanged.is_none());
    }

    #[test]
    fn tag_job_lifecycle_persists_usage_and_recovers_interrupted_work() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = Storage::new(temporary.path().join("data")).unwrap();
        let markdown = temporary.path().join("report.md");
        fs::write(&markdown, "# report").unwrap();
        let job = storage
            .put_tag_job(
                "profile",
                &markdown,
                Path::new("nested/report.md"),
                "schema-a",
                TagJobStatus::Queued,
                true,
            )
            .unwrap();

        storage
            .set_tag_job_status(&job.id, TagJobStatus::Reading, None)
            .unwrap();
        storage
            .update_tag_job_usage(&job.id, 512, 1024, 2, 120, 30)
            .unwrap();
        let active = storage.get_tag_job(&job.id).unwrap().unwrap();
        assert_eq!(active.status, TagJobStatus::Reading);
        assert_eq!((active.read_bytes, active.total_bytes), (512, 1024));
        assert_eq!(
            (active.api_calls, active.input_tokens, active.output_tokens),
            (2, 120, 30)
        );

        assert_eq!(storage.requeue_interrupted_tag_jobs().unwrap(), 1);
        let recovered = storage.get_tag_job(&job.id).unwrap().unwrap();
        assert_eq!(recovered.status, TagJobStatus::Queued);
        assert!(recovered.error.unwrap().contains("重新排队"));

        storage
            .complete_tag_job(
                &job.id,
                "content",
                r#"{"topics":["AI"]}"#,
                1024,
                1024,
                3,
                180,
                42,
            )
            .unwrap();
        let completed = storage.get_tag_job(&job.id).unwrap().unwrap();
        assert_eq!(completed.status, TagJobStatus::Completed);
        assert_eq!(completed.content_hash.as_deref(), Some("content"));
        assert_eq!(completed.api_calls, 3);
        assert_eq!(
            completed.result_json.as_deref(),
            Some(r#"{"topics":["AI"]}"#)
        );
    }

    #[test]
    fn legacy_completed_task_backfills_metadata_without_requeueing() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output = temporary.path().join("output");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        let source = input.join("legacy.docx");
        let result = output.join("legacy.md");
        fs::write(&source, b"legacy").unwrap();
        fs::write(&result, b"done").unwrap();
        let profile = WatchProfile {
            id: "legacy-profile".to_string(),
            name: "legacy-profile".to_string(),
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output.to_string_lossy().to_string(),
            enabled: true,
            delete_policy: Default::default(),
            tagging: Default::default(),
        };
        let storage = Storage::new(temporary.path().join("data")).unwrap();
        let task = storage
            .prepare_task(
                &profile,
                &source,
                Path::new("legacy.docx"),
                "legacy-hash",
                6,
                100,
                ConversionEngine::Anytomd,
                &result,
                true,
            )
            .unwrap()
            .unwrap();
        storage
            .set_status(&task.id, JobStatus::Completed, None)
            .unwrap();
        storage
            .open()
            .unwrap()
            .execute(
                "UPDATE tasks SET source_size=NULL, source_modified_ms=NULL WHERE id=?1",
                [&task.id],
            )
            .unwrap();

        let legacy = storage
            .queue_task(
                &profile,
                &source,
                Path::new("legacy.docx"),
                6,
                100,
                ConversionEngine::Anytomd,
                &result,
                false,
            )
            .unwrap();
        assert!(legacy.is_none());
        let backfilled = storage.get_task(&task.id).unwrap().unwrap();
        assert_eq!(backfilled.source_size, Some(6));
        assert_eq!(backfilled.source_modified_ms, Some(100));
        assert_eq!(backfilled.status, JobStatus::Completed);
    }

    #[test]
    fn deleting_a_profile_removes_only_its_task_history() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = Storage::new(temporary.path().join("data")).unwrap();
        let connection = storage.open().unwrap();
        for profile_id in ["remove", "keep"] {
            connection
                .execute(
                    "INSERT INTO tasks (id, profile_id, source_path, relative_path, engine, status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'a.docx', 'anytomd', 'completed', 'now', 'now')",
                    params![format!("task-{profile_id}"), profile_id, format!("{profile_id}/a.docx")],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO tag_jobs (id, profile_id, markdown_path, relative_path, status, schema_hash, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'a.md', 'completed', 'schema', 'now', 'now')",
                    params![format!("tag-{profile_id}"), profile_id, format!("{profile_id}/a.md")],
                )
                .unwrap();
        }

        storage
            .delete_profile_records(&["remove".to_string()])
            .unwrap();

        assert_eq!(storage.task_count().unwrap(), 1);
        assert_eq!(storage.tag_job_count().unwrap(), 1);
        assert_eq!(storage.list_tasks(10).unwrap()[0].profile_id, "keep");
        assert_eq!(storage.list_tag_jobs(10).unwrap()[0].profile_id, "keep");
    }
}
