use crate::converter::{ConversionArtifact, ConversionAsset, asset_reference_variants};
use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;
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
        let bytes = tokio::fs::read(source_path)
            .await
            .with_context(|| format!("无法读取待上传文件：{}", source_path.display()))?;
        self.http
            .put(upload_url)
            .body(bytes)
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

#[cfg(test)]
mod tests {
    use super::*;
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

    #[tokio::test]
    #[ignore = "requires CPAHDOCS_MINERU_E2E and a MinerU token in Windows Credential Manager"]
    async fn real_mineru_upload_poll_and_download() {
        let source = PathBuf::from(std::env::var("CPAHDOCS_MINERU_E2E").unwrap());
        let token = crate::state::AppState::read_mineru_token().unwrap();
        let base_url = std::env::var("CPAHDOCS_MINERU_BASE_URL")
            .unwrap_or_else(|_| "https://mineru.net/api/v4".to_string());
        let client = MinerUClient::new().unwrap();
        let submission = client.submit(&source, &base_url, &token).await.unwrap();
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
}
