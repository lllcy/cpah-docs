use crate::converter::asset_reference_variants;
#[cfg(test)]
use crate::converter::{ConversionArtifact, ConversionAsset};
use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
#[cfg(test)]
use std::io::Cursor;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;
use zip::ZipArchive;

const MAX_UPLOAD_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MARKDOWN_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ASSET_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TOTAL_ASSET_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 20_000;

#[derive(Clone)]
pub struct MinerUClient {
    http: Client,
}

#[derive(Debug, Clone)]
pub struct MinerUSubmission {
    pub batch_id: String,
    pub data_id: String,
    pub upload_url: String,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    code: i64,
    msg: Option<String>,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct UploadData {
    batch_id: String,
    file_urls: Vec<String>,
}

#[derive(Debug, Serialize)]
struct UploadRequest {
    files: Vec<UploadFileRequest>,
    enable_formula: bool,
    enable_table: bool,
    language: String,
    model_version: String,
}

#[derive(Debug, Serialize)]
struct UploadFileRequest {
    name: String,
    is_ocr: bool,
    data_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_ranges: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExtractData {
    #[serde(default)]
    extract_result: Vec<ExtractResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExtractResult {
    pub data_id: Option<String>,
    pub full_zip_url: Option<String>,
    pub full_markdown_url: Option<String>,
    pub err_msg: Option<String>,
    pub state: Option<String>,
    #[serde(default)]
    pub extract_progress: Option<ExtractProgress>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExtractProgress {
    pub extracted_pages: Option<i64>,
    pub total_pages: Option<i64>,
    pub start_time: Option<String>,
}

impl MinerUClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(Duration::from_secs(600))
                .build()?,
        })
    }

    pub async fn submit(
        &self,
        source_path: &Path,
        page_ranges: Option<&str>,
        base_url: &str,
        token: &str,
    ) -> Result<MinerUSubmission> {
        let data_id = Uuid::new_v4().simple().to_string();
        let file_name = source_path
            .file_name()
            .context("源文件缺少文件名")?
            .to_string_lossy()
            .to_string();
        let request = UploadRequest {
            files: vec![UploadFileRequest {
                name: file_name,
                is_ocr: true,
                data_id: data_id.clone(),
                page_ranges: page_ranges.map(str::to_string),
            }],
            enable_formula: true,
            enable_table: true,
            language: "ch".to_string(),
            model_version: "pipeline".to_string(),
        };
        let response = self
            .http
            .post(format!(
                "{}/file-urls/batch",
                base_url.trim_end_matches('/')
            ))
            .bearer_auth(token)
            .json(&request)
            .send()
            .await?
            .error_for_status()?
            .json::<ApiResponse<UploadData>>()
            .await?;
        let data = require_api_data(response, "申请上传地址")?;
        let upload_url = data
            .file_urls
            .into_iter()
            .next()
            .context("MinerU 未返回上传地址")?;
        Ok(MinerUSubmission {
            batch_id: data.batch_id,
            data_id,
            upload_url,
        })
    }

    pub async fn upload(&self, source_path: &Path, upload_url: &str) -> Result<()> {
        let size = tokio::fs::metadata(source_path).await?.len();
        if size > MAX_UPLOAD_BYTES {
            bail!("待上传文件超过 512 MiB 安全上限：{}", source_path.display());
        }
        let file = tokio::fs::File::open(source_path)
            .await
            .with_context(|| format!("无法打开待上传文件：{}", source_path.display()))?;
        self.http
            .put(upload_url)
            .header(reqwest::header::CONTENT_LENGTH, size)
            .body(reqwest::Body::wrap_stream(ReaderStream::new(file)))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn poll<F>(
        &self,
        batch_id: &str,
        data_id: Option<&str>,
        base_url: &str,
        token: &str,
        mut on_progress: F,
    ) -> Result<ExtractResult>
    where
        F: FnMut(&ExtractResult) -> Result<()>,
    {
        let endpoint = format!(
            "{}/extract-results/batch/{batch_id}",
            base_url.trim_end_matches('/')
        );
        for _ in 0..360 {
            let response = self
                .http
                .get(&endpoint)
                .bearer_auth(token)
                .send()
                .await?
                .error_for_status()?
                .json::<ApiResponse<ExtractData>>()
                .await?;
            let data = require_api_data(response, "查询解析结果")?;
            if let Some(item) = data
                .extract_result
                .into_iter()
                .find(|item| data_id.is_none() || item.data_id.as_deref() == data_id)
            {
                on_progress(&item)?;
                match item.state.as_deref().unwrap_or_default() {
                    "done" => return Ok(item),
                    "failed" => bail!(
                        "MinerU 解析失败：{}",
                        item.err_msg.as_deref().unwrap_or("未知错误")
                    ),
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        bail!("MinerU 解析超时（30 分钟）")
    }

    #[cfg(test)]
    pub async fn download(&self, result: &ExtractResult) -> Result<ConversionArtifact> {
        if let Some(zip_url) = result.full_zip_url.as_deref().filter(|url| !url.is_empty()) {
            let bytes = self
                .download_limited(zip_url, MAX_DOWNLOAD_BYTES, "ZIP")
                .await?;
            return tokio::task::spawn_blocking(move || extract_zip(bytes))
                .await
                .context("MinerU 解压任务异常")?;
        }
        if let Some(markdown_url) = result
            .full_markdown_url
            .as_deref()
            .filter(|url| !url.is_empty())
        {
            let markdown = String::from_utf8(
                self.download_limited(markdown_url, MAX_MARKDOWN_BYTES, "Markdown")
                    .await?,
            )
            .context("MinerU Markdown 不是 UTF-8")?;
            return Ok(ConversionArtifact {
                markdown,
                assets: Vec::new(),
                warnings: Vec::new(),
            });
        }
        bail!("MinerU 结果缺少 ZIP 和 Markdown 下载地址")
    }

    pub async fn download_to_stage(&self, result: &ExtractResult, stage_dir: &Path) -> Result<()> {
        if stage_dir.exists() {
            tokio::fs::remove_dir_all(stage_dir)
                .await
                .with_context(|| format!("无法清理 MinerU 暂存目录：{}", stage_dir.display()))?;
        }
        tokio::fs::create_dir_all(stage_dir)
            .await
            .with_context(|| format!("无法创建 MinerU 暂存目录：{}", stage_dir.display()))?;
        if let Some(zip_url) = result.full_zip_url.as_deref().filter(|url| !url.is_empty()) {
            let zip_path = stage_dir.join("result.zip");
            self.download_limited_to_path(zip_url, MAX_DOWNLOAD_BYTES, "ZIP", &zip_path)
                .await?;
            let zip_path_for_extract = zip_path.clone();
            let stage_for_extract = stage_dir.to_path_buf();
            tokio::task::spawn_blocking(move || {
                extract_zip_file_to_stage(&zip_path_for_extract, &stage_for_extract)
            })
            .await
            .context("MinerU 解压任务异常")??;
            tokio::fs::remove_file(&zip_path).await.ok();
            return Ok(());
        }
        if let Some(markdown_url) = result
            .full_markdown_url
            .as_deref()
            .filter(|url| !url.is_empty())
        {
            self.download_limited_to_path(
                markdown_url,
                MAX_MARKDOWN_BYTES,
                "Markdown",
                &stage_dir.join("full.md"),
            )
            .await?;
            return Ok(());
        }
        bail!("MinerU 结果缺少 ZIP 和 Markdown 下载地址")
    }

    async fn download_limited_to_path(
        &self,
        url: &str,
        limit: u64,
        label: &str,
        destination: &Path,
    ) -> Result<()> {
        let mut response = self.http.get(url).send().await?.error_for_status()?;
        if response
            .content_length()
            .is_some_and(|length| length > limit)
        {
            bail!("MinerU {label} 超过下载安全上限");
        }
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(destination)
            .await
            .with_context(|| format!("无法创建 MinerU {label} 暂存文件"))?;
        let mut total = 0_u64;
        while let Some(chunk) = response.chunk().await? {
            total = total
                .checked_add(chunk.len() as u64)
                .context("MinerU 下载大小溢出")?;
            if total > limit {
                bail!("MinerU {label} 超过下载安全上限");
            }
            file.write_all(&chunk).await?;
        }
        file.sync_all().await?;
        Ok(())
    }

    #[cfg(test)]
    async fn download_limited(&self, url: &str, limit: u64, label: &str) -> Result<Vec<u8>> {
        let mut response = self.http.get(url).send().await?.error_for_status()?;
        if response
            .content_length()
            .is_some_and(|length| length > limit)
        {
            bail!("MinerU {label} 超过下载安全上限");
        }
        let capacity = response.content_length().unwrap_or(0).min(limit) as usize;
        let mut bytes = Vec::with_capacity(capacity);
        while let Some(chunk) = response.chunk().await? {
            if (bytes.len() as u64).saturating_add(chunk.len() as u64) > limit {
                bail!("MinerU {label} 超过下载安全上限");
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

pub fn staged_markdown_path(stage_dir: &Path) -> PathBuf {
    stage_dir.join("full.md")
}

pub fn staged_assets_path(stage_dir: &Path) -> PathBuf {
    stage_dir.join("assets")
}

fn require_api_data<T>(response: ApiResponse<T>, action: &str) -> Result<T> {
    if response.code != 0 {
        bail!(
            "MinerU {action}失败（code={}）：{}",
            response.code,
            response.msg.as_deref().unwrap_or("未知错误")
        );
    }
    response
        .data
        .context(format!("MinerU {action}响应缺少 data"))
}

#[cfg(test)]
fn extract_zip(bytes: Vec<u8>) -> Result<ConversionArtifact> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).context("MinerU 返回的 ZIP 无效")?;
    if archive.len() > MAX_ZIP_ENTRIES {
        bail!("MinerU ZIP 文件条目过多（最多 {MAX_ZIP_ENTRIES} 个）");
    }
    let mut fallback_markdown = None;
    let mut markdown_index = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        if path.file_name().is_some_and(|name| name == "full.md") {
            markdown_index = Some(index);
            break;
        }
        if fallback_markdown.is_none()
            && path.extension().is_some_and(|extension| extension == "md")
        {
            fallback_markdown = Some(index);
        }
    }
    let markdown_index = markdown_index
        .or(fallback_markdown)
        .context("MinerU ZIP 中没有 Markdown 文件")?;
    let (markdown_path, markdown_bytes) = {
        let entry = archive.by_index(markdown_index)?;
        if entry.size() > MAX_MARKDOWN_BYTES {
            bail!("MinerU Markdown 超过 64 MiB 安全上限");
        }
        let path = entry
            .enclosed_name()
            .context("MinerU Markdown 路径不安全")?;
        let mut content = Vec::with_capacity(entry.size() as usize);
        entry
            .take(MAX_MARKDOWN_BYTES + 1)
            .read_to_end(&mut content)?;
        if content.len() as u64 > MAX_MARKDOWN_BYTES {
            bail!("MinerU Markdown 超过 64 MiB 安全上限");
        }
        (path, content)
    };
    let markdown = String::from_utf8(markdown_bytes).context("MinerU Markdown 不是 UTF-8")?;
    let markdown_parent = markdown_path.parent().unwrap_or_else(|| Path::new(""));
    let mut assets = Vec::new();
    let mut total_asset_bytes = 0_u64;
    for index in 0..archive.len() {
        if index == markdown_index {
            continue;
        }
        let entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let Some(relative) = path
            .strip_prefix(markdown_parent)
            .ok()
            .map(Path::to_path_buf)
            .or_else(|| path.file_name().map(PathBuf::from))
        else {
            continue;
        };
        if !asset_reference_variants(&relative)
            .iter()
            .any(|reference| markdown.contains(reference))
        {
            continue;
        }
        if entry.size() > MAX_ASSET_BYTES {
            bail!(
                "MinerU 单个附件超过 128 MiB 安全上限：{}",
                relative.display()
            );
        }
        let mut content = Vec::with_capacity(entry.size() as usize);
        entry.take(MAX_ASSET_BYTES + 1).read_to_end(&mut content)?;
        if content.len() as u64 > MAX_ASSET_BYTES {
            bail!(
                "MinerU 单个附件超过 128 MiB 安全上限：{}",
                relative.display()
            );
        }
        total_asset_bytes = total_asset_bytes
            .checked_add(content.len() as u64)
            .context("MinerU 附件大小溢出")?;
        if total_asset_bytes > MAX_TOTAL_ASSET_BYTES {
            bail!("MinerU 附件合计超过 512 MiB 安全上限");
        }
        assets.push(ConversionAsset {
            relative_path: relative,
            bytes: content,
        });
    }
    Ok(ConversionArtifact {
        markdown,
        assets,
        warnings: Vec::new(),
    })
}

fn extract_zip_file_to_stage(zip_path: &Path, stage_dir: &Path) -> Result<()> {
    let file = File::open(zip_path)
        .with_context(|| format!("无法打开 MinerU ZIP：{}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file).context("MinerU 返回的 ZIP 无效")?;
    extract_archive_to_stage(&mut archive, stage_dir)
}

fn extract_archive_to_stage<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    stage_dir: &Path,
) -> Result<()> {
    if archive.len() > MAX_ZIP_ENTRIES {
        bail!("MinerU ZIP 文件条目过多（最多 {MAX_ZIP_ENTRIES} 个）");
    }
    let mut fallback_markdown = None;
    let mut markdown_index = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        if path.file_name().is_some_and(|name| name == "full.md") {
            markdown_index = Some(index);
            break;
        }
        if fallback_markdown.is_none()
            && path.extension().is_some_and(|extension| extension == "md")
        {
            fallback_markdown = Some(index);
        }
    }
    let markdown_index = markdown_index
        .or(fallback_markdown)
        .context("MinerU ZIP 中没有 Markdown 文件")?;
    let (markdown_path, markdown) = {
        let entry = archive.by_index(markdown_index)?;
        if entry.size() > MAX_MARKDOWN_BYTES {
            bail!("MinerU Markdown 超过 64 MiB 安全上限");
        }
        let path = entry
            .enclosed_name()
            .context("MinerU Markdown 路径不安全")?;
        let mut content = Vec::with_capacity(entry.size() as usize);
        entry
            .take(MAX_MARKDOWN_BYTES + 1)
            .read_to_end(&mut content)?;
        if content.len() as u64 > MAX_MARKDOWN_BYTES {
            bail!("MinerU Markdown 超过 64 MiB 安全上限");
        }
        let markdown = String::from_utf8(content).context("MinerU Markdown 不是 UTF-8")?;
        (path, markdown)
    };
    crate::atomic_file::write_atomic(&staged_markdown_path(stage_dir), markdown.as_bytes())?;
    let markdown_parent = markdown_path.parent().unwrap_or_else(|| Path::new(""));
    let assets_root = staged_assets_path(stage_dir);
    let mut total_asset_bytes = 0_u64;
    for index in 0..archive.len() {
        if index == markdown_index {
            continue;
        }
        let entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let Some(relative) = path
            .strip_prefix(markdown_parent)
            .ok()
            .map(Path::to_path_buf)
            .or_else(|| path.file_name().map(PathBuf::from))
        else {
            continue;
        };
        if !asset_reference_variants(&relative)
            .iter()
            .any(|reference| markdown.contains(reference))
        {
            continue;
        }
        if entry.size() > MAX_ASSET_BYTES {
            bail!(
                "MinerU 单个附件超过 128 MiB 安全上限：{}",
                relative.display()
            );
        }
        let declared_total = total_asset_bytes
            .checked_add(entry.size())
            .context("MinerU 附件大小溢出")?;
        if declared_total > MAX_TOTAL_ASSET_BYTES {
            bail!("MinerU 附件合计超过 512 MiB 安全上限");
        }
        let destination = assets_root.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&destination)
            .with_context(|| format!("无法暂存 MinerU 附件：{}", destination.display()))?;
        let written = std::io::copy(&mut entry.take(MAX_ASSET_BYTES + 1), &mut output)?;
        if written > MAX_ASSET_BYTES {
            bail!(
                "MinerU 单个附件超过 128 MiB 安全上限：{}",
                relative.display()
            );
        }
        total_asset_bytes = total_asset_bytes
            .checked_add(written)
            .context("MinerU 附件大小溢出")?;
        if total_asset_bytes > MAX_TOTAL_ASSET_BYTES {
            bail!("MinerU 附件合计超过 512 MiB 安全上限");
        }
        output.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::{StagedMinerUPart, write_multipart_artifact};
    use crate::models::{
        ConversionEngine, JobStatus, MinerUPartMode, TaskKind, TaskRecord, WatchProfile,
    };
    use chrono::Utc;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut archive = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        for (name, bytes) in entries {
            archive.start_file(*name, options).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn extract_zip_keeps_only_assets_referenced_by_markdown() {
        let bytes = test_zip(&[
            (
                "result/full.md",
                b"![](images/used.jpg)\n<img src=\"images/chart%20one.png\">",
            ),
            ("result/images/used.jpg", b"used"),
            ("result/images/chart one.png", b"chart"),
            ("result/images/unused.jpg", b"unused"),
            ("result/content_list.json", b"{}"),
            ("result/origin.pdf", b"%PDF-test"),
        ]);

        let artifact = extract_zip(bytes).unwrap();
        let paths = artifact
            .assets
            .iter()
            .map(|asset| asset.relative_path.to_string_lossy().replace('\\', "/"))
            .collect::<Vec<_>>();

        assert_eq!(paths, vec!["images/used.jpg", "images/chart one.png"]);
        assert_eq!(artifact.assets[0].bytes, b"used");
        assert_eq!(artifact.assets[1].bytes, b"chart");
    }

    #[test]
    fn staged_zip_extraction_keeps_only_safe_referenced_assets() {
        let temporary = tempfile::tempdir().unwrap();
        let zip_path = temporary.path().join("result.zip");
        let stage = temporary.path().join("stage");
        fs::create_dir_all(&stage).unwrap();
        fs::write(
            &zip_path,
            test_zip(&[
                ("result/full.md", b"![](images/used.jpg)"),
                ("result/images/used.jpg", b"used"),
                ("result/images/unused.jpg", b"unused"),
                ("../escape.txt", b"escape"),
            ]),
        )
        .unwrap();

        extract_zip_file_to_stage(&zip_path, &stage).unwrap();

        assert_eq!(
            fs::read_to_string(stage.join("full.md")).unwrap(),
            "![](images/used.jpg)"
        );
        assert_eq!(
            fs::read(stage.join("assets/images/used.jpg")).unwrap(),
            b"used"
        );
        assert!(!stage.join("assets/images/unused.jpg").exists());
        assert!(!temporary.path().join("escape.txt").exists());
    }

    #[test]
    fn reads_page_progress_from_batch_response() {
        let response: ApiResponse<ExtractData> = serde_json::from_str(
            r#"{
                "code": 0,
                "msg": "ok",
                "data": {
                    "extract_result": [{
                        "data_id": "doc-1",
                        "state": "running",
                        "extract_progress": {
                            "extracted_pages": 7,
                            "total_pages": 20,
                            "start_time": "2026-08-11 10:30:00"
                        }
                    }]
                }
            }"#,
        )
        .unwrap();
        let item = response.data.unwrap().extract_result.remove(0);
        let progress = item.extract_progress.unwrap();

        assert_eq!(item.state.as_deref(), Some("running"));
        assert_eq!(progress.extracted_pages, Some(7));
        assert_eq!(progress.total_pages, Some(20));
        assert_eq!(progress.start_time.as_deref(), Some("2026-08-11 10:30:00"));
    }

    #[test]
    fn page_ranges_is_serialized_on_the_individual_upload_file() {
        let request = UploadRequest {
            files: vec![UploadFileRequest {
                name: "large.pdf".to_string(),
                is_ocr: true,
                data_id: "part-1".to_string(),
                page_ranges: Some("1-200".to_string()),
            }],
            enable_formula: true,
            enable_table: true,
            language: "ch".to_string(),
            model_version: "pipeline".to_string(),
        };
        let json = serde_json::to_value(request).unwrap();
        assert_eq!(json["files"][0]["page_ranges"], "1-200");
        assert!(json.get("page_ranges").is_none());
    }

    #[tokio::test]
    #[ignore = "requires CPAHDOCS_MINERU_E2E and CPAHDOCS_MINERU_TOKEN or a saved MinerU token"]
    async fn real_mineru_upload_poll_and_download() {
        let source = PathBuf::from(std::env::var("CPAHDOCS_MINERU_E2E").unwrap());
        let token = std::env::var("CPAHDOCS_MINERU_TOKEN")
            .or_else(|_| crate::state::AppState::read_mineru_token())
            .unwrap();
        let base_url = std::env::var("CPAHDOCS_MINERU_BASE_URL")
            .unwrap_or_else(|_| "https://mineru.net/api/v4".to_string());
        let client = MinerUClient::new().unwrap();
        let submission = client
            .submit(&source, None, &base_url, &token)
            .await
            .unwrap();
        client
            .upload(&source, &submission.upload_url)
            .await
            .unwrap();
        let result = client
            .poll(
                &submission.batch_id,
                Some(&submission.data_id),
                &base_url,
                &token,
                |_| Ok(()),
            )
            .await
            .unwrap();
        let artifact = client.download(&result).await.unwrap();
        eprintln!(
            "mineru e2e: markdown_bytes={} assets={} warnings={}",
            artifact.markdown.len(),
            artifact.assets.len(),
            artifact.warnings.len()
        );
        assert!(!artifact.markdown.trim().is_empty());
    }

    #[tokio::test]
    #[ignore = "requires CPAHDOCS_MINERU_PAGE_RANGES_E2E (>200-page PDF) and MinerU credentials"]
    async fn real_mineru_page_ranges_split() {
        let source = PathBuf::from(std::env::var("CPAHDOCS_MINERU_PAGE_RANGES_E2E").unwrap());
        run_real_multipart_e2e(&source, MinerUPartMode::PageRanges).await;
    }

    #[tokio::test]
    #[ignore = "requires CPAHDOCS_MINERU_PHYSICAL_E2E (200-512 MiB PDF) and MinerU credentials"]
    async fn real_mineru_physical_pdf_split() {
        let source = PathBuf::from(std::env::var("CPAHDOCS_MINERU_PHYSICAL_E2E").unwrap());
        run_real_multipart_e2e(&source, MinerUPartMode::SplitPdf).await;
    }

    async fn run_real_multipart_e2e(source: &Path, expected_mode: MinerUPartMode) {
        use crate::pdf_split::{PdfPlan, plan_pdf};

        let temporary = tempfile::tempdir().unwrap();
        let plan = plan_pdf(source, &temporary.path().join("split-work")).unwrap();
        let PdfPlan::Multipart { page_count, parts } = plan else {
            panic!("multipart E2E source did not require splitting")
        };
        assert!(parts.len() > 1, "multipart E2E requires at least two parts");
        assert!(parts.iter().all(|part| part.mode == expected_mode));

        let token = std::env::var("CPAHDOCS_MINERU_TOKEN")
            .or_else(|_| crate::state::AppState::read_mineru_token())
            .unwrap();
        let base_url = std::env::var("CPAHDOCS_MINERU_BASE_URL")
            .unwrap_or_else(|_| "https://mineru.net/api/v4".to_string());
        let client = MinerUClient::new().unwrap();

        let stages_root = temporary.path().join("stages");
        let mut staged_parts = Vec::with_capacity(parts.len());
        for part in &parts {
            let input = part.input_path.as_deref().unwrap_or(source);
            let page_ranges = (part.mode == MinerUPartMode::PageRanges)
                .then(|| format!("{}-{}", part.page_start, part.page_end));
            let stage_dir = stages_root.join(format!("part-{:04}", part.index));
            run_real_staged_e2e(
                &client,
                input,
                page_ranges.as_deref(),
                part.mode == MinerUPartMode::SplitPdf,
                &stage_dir,
                &base_url,
                &token,
            )
            .await;
            staged_parts.push(StagedMinerUPart {
                index: i64::from(part.index),
                page_start: i64::from(part.page_start),
                page_end: i64::from(part.page_end),
                stage_dir,
            });
        }

        let output_root = std::env::var_os("CPAHDOCS_MINERU_E2E_OUTPUT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| temporary.path().join("output"));
        fs::create_dir_all(&output_root).unwrap();
        let output = output_root.join(format!(
            "{}-merged.md",
            source.file_stem().unwrap().to_string_lossy()
        ));
        let profile = WatchProfile {
            id: "mineru-e2e".to_string(),
            name: "MinerU E2E".to_string(),
            input_dir: source.parent().unwrap().to_string_lossy().to_string(),
            output_dir: output_root.to_string_lossy().to_string(),
            enabled: true,
            delete_policy: Default::default(),
            tagging: Default::default(),
        };
        let task = TaskRecord {
            id: "mineru-e2e-parent".to_string(),
            kind: TaskKind::Document,
            parent_task_id: None,
            part_index: None,
            part_count: Some(parts.len() as i64),
            page_start: None,
            page_end: None,
            part_mode: None,
            part_completed_count: Some(parts.len() as i64),
            part_failed_count: Some(0),
            profile_id: profile.id.clone(),
            source_path: source.to_string_lossy().to_string(),
            relative_path: source.file_name().unwrap().to_string_lossy().to_string(),
            source_hash: Some("mineru-e2e".to_string()),
            source_size: Some(fs::metadata(source).unwrap().len() as i64),
            source_modified_ms: None,
            engine: ConversionEngine::Mineru,
            status: JobStatus::Converting,
            output_path: Some(output.to_string_lossy().to_string()),
            error: None,
            error_code: None,
            error_title: None,
            error_suggestion: None,
            mineru_batch_id: None,
            mineru_data_id: None,
            mineru_state: Some("done".to_string()),
            mineru_extracted_pages: Some(i64::from(page_count)),
            mineru_total_pages: Some(i64::from(page_count)),
            mineru_started_at: None,
            updated_at: Utc::now().to_rfc3339(),
            tag_job_id: None,
            tag_status: None,
        };
        write_multipart_artifact(&profile, &task, &staged_parts).unwrap();

        let markdown = fs::read_to_string(&output).unwrap();
        assert_eq!(markdown.lines().filter(|line| *line == "---").count(), 2);
        for part in &parts {
            assert!(markdown.contains(&format!(
                "<!-- cpah-docs: MinerU source pages {}-{} -->",
                part.page_start, part.page_end
            )));
        }
        assert!(
            parts
                .windows(2)
                .all(|pair| pair[0].page_end + 1 == pair[1].page_start)
        );
        eprintln!(
            "mineru multipart e2e: mode={} pages={} parts={} markdown_bytes={} output={}",
            expected_mode.as_str(),
            page_count,
            parts.len(),
            markdown.len(),
            output.display()
        );
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_real_staged_e2e(
        client: &MinerUClient,
        source: &Path,
        page_ranges: Option<&str>,
        delete_after_upload: bool,
        stage_dir: &Path,
        base_url: &str,
        token: &str,
    ) {
        let label = page_ranges.unwrap_or("physical-part");
        eprintln!(
            "mineru e2e submit: label={} bytes={}",
            label,
            fs::metadata(source).unwrap().len()
        );
        let submission = client
            .submit(source, page_ranges, base_url, token)
            .await
            .unwrap();
        eprintln!("mineru e2e upload: label={label}");
        client.upload(source, &submission.upload_url).await.unwrap();
        eprintln!("mineru e2e upload complete: label={label}");
        if delete_after_upload {
            fs::remove_file(source).unwrap();
            assert!(!source.exists());
        }
        let mut last_progress = None;
        let result = client
            .poll(
                &submission.batch_id,
                Some(&submission.data_id),
                base_url,
                token,
                |item| {
                    let progress = (
                        item.state.clone().unwrap_or_default(),
                        item.extract_progress
                            .as_ref()
                            .and_then(|value| value.extracted_pages),
                        item.extract_progress
                            .as_ref()
                            .and_then(|value| value.total_pages),
                    );
                    if last_progress.as_ref() != Some(&progress) {
                        eprintln!(
                            "mineru e2e progress: label={} state={} pages={:?}/{:?}",
                            label, progress.0, progress.1, progress.2
                        );
                        last_progress = Some(progress);
                    }
                    Ok(())
                },
            )
            .await
            .unwrap();
        client.download_to_stage(&result, stage_dir).await.unwrap();
        let markdown = fs::read_to_string(stage_dir.join("full.md")).unwrap();
        eprintln!(
            "mineru e2e download complete: label={} markdown_bytes={}",
            label,
            markdown.len()
        );
        assert!(!markdown.trim().is_empty());
    }
}
