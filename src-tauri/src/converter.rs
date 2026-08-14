use crate::models::{ConversionEngine, TaskRecord, WatchProfile};
use anyhow::{Context, Result, bail};
use anytomd::{ConversionOptions, convert_bytes, convert_file};
use chrono::Utc;
use std::ffi::OsString;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

const LOCAL_EXTENSIONS: &[&str] = &[
    "md", "docx", "xlsx", "xls", "pptx", "html", "htm", "csv", "txt",
];
const MINERU_EXTENSIONS: &[&str] = &["pdf", "doc", "ppt", "png", "jpg", "jpeg", "webp", "bmp"];

#[derive(Debug, Clone)]
pub struct ConversionAsset {
    pub relative_path: PathBuf,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ConversionArtifact {
    pub markdown: String,
    pub assets: Vec<ConversionAsset>,
    pub warnings: Vec<String>,
}

pub fn is_supported(path: &Path) -> bool {
    extension(path).is_some_and(|value| {
        LOCAL_EXTENSIONS.contains(&value) || MINERU_EXTENSIONS.contains(&value)
    })
}

pub fn is_enabled(path: &Path, enabled_extensions: &[String]) -> bool {
    extension(path).is_some_and(|value| {
        enabled_extensions
            .iter()
            .any(|enabled| enabled.eq_ignore_ascii_case(value))
    })
}

pub fn default_engine(path: &Path) -> Option<ConversionEngine> {
    let ext = extension(path)?;
    if LOCAL_EXTENSIONS.contains(&ext) {
        Some(ConversionEngine::Anytomd)
    } else if MINERU_EXTENSIONS.contains(&ext) {
        Some(ConversionEngine::Mineru)
    } else {
        None
    }
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("md"))
}

fn extension(path: &Path) -> Option<&str> {
    path.extension()?
        .to_str()
        .map(str::to_ascii_lowercase)
        .and_then(|value| {
            LOCAL_EXTENSIONS
                .iter()
                .chain(MINERU_EXTENSIONS.iter())
                .copied()
                .find(|candidate| *candidate == value)
        })
}

pub fn output_path(profile: &WatchProfile, source_path: &Path) -> Result<PathBuf> {
    let input_root = Path::new(&profile.input_dir);
    let relative = source_path
        .strip_prefix(input_root)
        .with_context(|| format!("文件不在监控目录内：{}", source_path.display()))?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        bail!("源文件相对路径不安全：{}", relative.display());
    }
    let file_stem = relative
        .file_stem()
        .context("源文件缺少文件名")?
        .to_string_lossy();
    let output_name = if is_markdown(source_path) {
        relative
            .file_name()
            .context("Markdown 文件缺少文件名")?
            .to_string_lossy()
            .to_string()
    } else if has_stem_collision(source_path)? {
        let file_name = relative
            .file_name()
            .context("源文件缺少文件名")?
            .to_string_lossy();
        format!("{file_name}.md")
    } else {
        format!("{file_stem}.md")
    };
    let mut path = PathBuf::from(&profile.output_dir);
    if let Some(parent) = relative.parent() {
        path.push(parent);
    }
    path.push(output_name);
    Ok(path)
}

pub fn copy_markdown(profile: &WatchProfile, source_path: &Path, output: &Path) -> Result<PathBuf> {
    if !is_markdown(source_path) {
        bail!("直通同步只支持 Markdown 文件：{}", source_path.display());
    }
    ensure_output_is_safe(profile, output)?;
    if source_path == output {
        bail!("Markdown 输入和输出路径不能相同");
    }
    let output_parent = output.parent().context("输出文件缺少父目录")?;
    fs::create_dir_all(output_parent)?;
    crate::atomic_file::copy_atomic(source_path, output)
        .with_context(|| format!("无法同步 Markdown：{}", source_path.display()))?;
    Ok(output.to_path_buf())
}

fn has_stem_collision(source_path: &Path) -> Result<bool> {
    let parent = source_path.parent().context("源文件缺少父目录")?;
    let stem = source_path
        .file_stem()
        .context("源文件缺少文件名")?
        .to_string_lossy();
    let mut matches = 0;
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_file() || !is_supported(&path) {
            continue;
        }
        if path
            .file_stem()
            .is_some_and(|candidate| candidate.to_string_lossy().eq_ignore_ascii_case(&stem))
        {
            matches += 1;
            if matches > 1 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn convert_locally(source_path: &Path) -> Result<ConversionArtifact> {
    let options = ConversionOptions {
        extract_images: true,
        extract_comments: false,
        max_total_image_bytes: 256 * 1024 * 1024,
        max_input_bytes: 512 * 1024 * 1024,
        max_uncompressed_zip_bytes: 2 * 1024 * 1024 * 1024,
        strict: false,
        image_describer: None,
    };
    let mut result = convert_file(source_path, &options)
        .with_context(|| format!("anytomd 转换失败：{}", source_path.display()))?;
    if result.markdown.trim().is_empty()
        && source_path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("pptx"))
        && let Some(normalized) = normalize_pptx_slide_relationships(source_path)?
    {
        result = convert_bytes(&normalized, "pptx", &options).with_context(|| {
            format!(
                "anytomd 转换规范化后的 PPTX 失败：{}",
                source_path.display()
            )
        })?;
    }
    if result.markdown.trim().is_empty() {
        bail!("anytomd 未产生 Markdown 内容");
    }
    Ok(ConversionArtifact {
        markdown: result.markdown,
        assets: result
            .images
            .into_iter()
            .map(|(name, bytes)| ConversionAsset {
                relative_path: PathBuf::from(name),
                bytes,
            })
            .collect(),
        warnings: result
            .warnings
            .into_iter()
            .map(|warning| warning.message)
            .collect(),
    })
}

fn normalize_pptx_slide_relationships(source_path: &Path) -> Result<Option<Vec<u8>>> {
    let source = fs::read(source_path)
        .with_context(|| format!("无法读取 PPTX：{}", source_path.display()))?;
    let mut archive = ZipArchive::new(Cursor::new(source)).context("PPTX ZIP 结构无效")?;
    let relationship_path = "ppt/_rels/presentation.xml.rels";
    let Some(relationship_index) = archive.index_for_name(relationship_path) else {
        return Ok(None);
    };
    let relationship_xml = {
        let mut entry = archive.by_index(relationship_index)?;
        let mut content = String::new();
        entry.read_to_string(&mut content)?;
        content
    };
    let normalized_relationships = relationship_xml
        .replace("Target=\"/ppt/slides/", "Target=\"ppt/slides/")
        .replace("Target='/ppt/slides/", "Target='ppt/slides/");
    if normalized_relationships == relationship_xml {
        return Ok(None);
    }

    let cursor = Cursor::new(Vec::with_capacity(archive.offset() as usize));
    let mut writer = ZipWriter::new(cursor);
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let options = SimpleFileOptions::default()
            .compression_method(entry.compression())
            .last_modified_time(entry.last_modified().unwrap_or_default());
        let name = entry.name().to_string();
        if entry.is_dir() {
            writer.add_directory(name, options)?;
            continue;
        }
        writer.start_file(&name, options)?;
        if name == relationship_path {
            writer.write_all(normalized_relationships.as_bytes())?;
        } else {
            std::io::copy(&mut entry, &mut writer)?;
        }
    }
    Ok(Some(writer.finish()?.into_inner()))
}

pub fn write_artifact(
    profile: &WatchProfile,
    task: &TaskRecord,
    artifact: ConversionArtifact,
) -> Result<PathBuf> {
    let output = task
        .output_path
        .as_ref()
        .map(PathBuf::from)
        .context("任务缺少输出路径")?;
    ensure_output_is_safe(profile, &output)?;
    let output_parent = output.parent().context("输出文件缺少父目录")?;
    fs::create_dir_all(output_parent)?;

    let asset_dir = asset_path_for_output(&output).context("无法确定附件目录")?;
    let asset_dir_name = asset_dir
        .file_name()
        .context("附件目录缺少名称")?
        .to_string_lossy()
        .to_string();
    ensure_output_is_safe(profile, &asset_dir)?;

    let temporary_asset_dir =
        output_parent.join(format!(".{asset_dir_name}.tmp-{}", Uuid::new_v4().simple()));
    let mut markdown = artifact.markdown;

    if !artifact.assets.is_empty() {
        fs::create_dir_all(&temporary_asset_dir)?;
        for asset in &artifact.assets {
            let safe_relative = sanitize_relative_path(&asset.relative_path)?;
            let destination = temporary_asset_dir.join(&safe_relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&destination, &asset.bytes)?;
            markdown = rewrite_asset_reference(
                markdown,
                &asset.relative_path,
                &safe_relative,
                &asset_dir_name,
            );
        }
    }

    let frontmatter = build_frontmatter(task, &artifact.warnings)?;
    let complete = format!("{frontmatter}\n{}", markdown.trim_start());
    let asset_backup = if artifact.assets.is_empty() {
        None
    } else {
        Some(install_staged_assets(&temporary_asset_dir, &asset_dir)?)
    };
    if let Err(error) = crate::atomic_file::write_atomic(&output, complete.as_bytes()) {
        if let Some(backup) = asset_backup {
            rollback_staged_assets(&asset_dir, backup.as_deref())?;
        }
        return Err(error).context("无法原子写入转换结果");
    }

    if artifact.assets.is_empty() {
        if asset_dir.exists() {
            fs::remove_dir_all(&asset_dir)?;
        }
    } else if let Some(Some(backup)) = asset_backup
        && let Err(error) = fs::remove_dir_all(&backup)
    {
        tracing::warn!(error = %error, "failed to clean previous asset backup");
    }

    Ok(output)
}

fn install_staged_assets(staged: &Path, destination: &Path) -> Result<Option<PathBuf>> {
    let backup = destination.with_file_name(format!(
        ".{}.backup-{}",
        destination
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("assets"))
            .to_string_lossy(),
        Uuid::new_v4().simple()
    ));
    let previous = if destination.exists() {
        fs::rename(destination, &backup)
            .with_context(|| format!("无法暂存上一版附件目录：{}", destination.display()))?;
        Some(backup)
    } else {
        None
    };
    if let Err(error) = fs::rename(staged, destination) {
        if let Some(previous) = previous.as_deref() {
            let _ = fs::rename(previous, destination);
        }
        return Err(error)
            .with_context(|| format!("无法安装新附件目录：{}", destination.display()));
    }
    Ok(previous)
}

fn rollback_staged_assets(destination: &Path, backup: Option<&Path>) -> Result<()> {
    if destination.exists() {
        fs::remove_dir_all(destination).context("无法回滚新附件目录")?;
    }
    if let Some(backup) = backup {
        fs::rename(backup, destination).context("无法恢复上一版附件目录")?;
    }
    Ok(())
}

pub fn asset_path_for_output(output: &Path) -> Option<PathBuf> {
    let file_name = output.file_name()?.to_string_lossy();
    let source_name = file_name.strip_suffix(".md").unwrap_or(&file_name);
    Some(output.with_file_name(format!("{source_name}.assets")))
}

pub fn remove_generated_output(
    profile: &WatchProfile,
    output: &Path,
    to_trash: bool,
) -> Result<()> {
    ensure_output_is_safe(profile, output)?;
    let assets = asset_path_for_output(output);
    if to_trash {
        let relative = output
            .strip_prefix(&profile.output_dir)
            .context("输出文件不在输出目录中")?;
        let trash_root = PathBuf::from(&profile.output_dir)
            .join(".trash")
            .join(format!(
                "{}-{}",
                Utc::now().format("%Y%m%d-%H%M%S%.3f"),
                Uuid::new_v4().simple()
            ));
        let trash_output = trash_root.join(relative);
        if let Some(parent) = trash_output.parent() {
            fs::create_dir_all(parent)?;
        }
        if output.exists() {
            fs::rename(output, &trash_output)?;
        }
        if let Some(assets) = assets.filter(|path| path.exists()) {
            let asset_name = assets.file_name().context("附件目录缺少名称")?;
            fs::rename(&assets, trash_output.with_file_name(asset_name))?;
        }
    } else {
        if output.exists() {
            fs::remove_file(output)?;
        }
        if let Some(assets) = assets.filter(|path| path.exists()) {
            fs::remove_dir_all(assets)?;
        }
    }
    Ok(())
}

fn build_frontmatter(task: &TaskRecord, warnings: &[String]) -> Result<String> {
    let relative = serde_json::to_string(&task.relative_path)?;
    let hash = serde_json::to_string(task.source_hash.as_deref().unwrap_or_default())?;
    let warning_json = serde_json::to_string(warnings)?;
    Ok(format!(
        "---\nsource: {relative}\nsource_sha256: {hash}\nconverter: {}\nconverted_at: {}\nwarnings: {warning_json}\n---\n",
        task.engine.as_str(),
        Utc::now().to_rfc3339(),
    ))
}

fn sanitize_relative_path(path: &Path) -> Result<PathBuf> {
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => safe.push(value),
            Component::CurDir => {}
            _ => bail!("附件路径不安全：{}", path.display()),
        }
    }
    if safe.as_os_str().is_empty() {
        safe.push(OsString::from(format!("asset-{}", Uuid::new_v4().simple())));
    }
    Ok(safe)
}

fn rewrite_asset_reference(
    markdown: String,
    original: &Path,
    safe: &Path,
    asset_dir_name: &str,
) -> String {
    let safe = safe.to_string_lossy().replace('\\', "/");
    let replacement_path = format!("{asset_dir_name}/{safe}");
    let encoded_replacement_path = percent_encode_path(&replacement_path);
    let mut rewritten = markdown;
    for original in asset_reference_variants(original) {
        for candidate in [original.clone(), format!("./{original}")] {
            rewritten = rewritten
                .replace(
                    &format!("]({candidate})"),
                    &format!("](<{replacement_path}>)"),
                )
                .replace(
                    &format!("](<{candidate}>)"),
                    &format!("](<{replacement_path}>)"),
                )
                .replace(
                    &format!("src=\"{candidate}\""),
                    &format!("src=\"{encoded_replacement_path}\""),
                )
                .replace(
                    &format!("src='{candidate}'"),
                    &format!("src='{encoded_replacement_path}'"),
                )
                .replace(
                    &format!("href=\"{candidate}\""),
                    &format!("href=\"{encoded_replacement_path}\""),
                )
                .replace(
                    &format!("href='{candidate}'"),
                    &format!("href='{encoded_replacement_path}'"),
                );
        }
    }
    rewritten
}

pub(crate) fn asset_reference_variants(path: &Path) -> Vec<String> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let encoded = percent_encode_path(&normalized);
    if encoded == normalized {
        vec![normalized]
    } else {
        vec![normalized, encoded]
    }
}

fn percent_encode_path(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(&mut encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn ensure_output_is_safe(profile: &WatchProfile, path: &Path) -> Result<()> {
    let root = Path::new(&profile.output_dir);
    if !path.starts_with(root) || path == root {
        bail!("拒绝操作输出目录之外的路径：{}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> WatchProfile {
        WatchProfile {
            id: "one".to_string(),
            name: "test".to_string(),
            input_dir: if cfg!(windows) { "C:\\input" } else { "/input" }.to_string(),
            output_dir: if cfg!(windows) {
                "C:\\output"
            } else {
                "/output"
            }
            .to_string(),
            enabled: true,
            delete_policy: Default::default(),
            tagging: Default::default(),
        }
    }

    #[test]
    fn routes_core_formats() {
        assert_eq!(
            default_engine(Path::new("a.docx")),
            Some(ConversionEngine::Anytomd)
        );
        assert_eq!(
            default_engine(Path::new("a.pdf")),
            Some(ConversionEngine::Mineru)
        );
        assert_eq!(
            default_engine(Path::new("a.MD")),
            Some(ConversionEngine::Anytomd)
        );
        assert_eq!(default_engine(Path::new("a.exe")), None);
    }

    #[test]
    fn format_switches_are_case_insensitive() {
        let enabled = vec!["docx".to_string(), "PDF".to_string()];
        assert!(is_enabled(Path::new("report.DOCX"), &enabled));
        assert!(is_enabled(Path::new("report.pdf"), &enabled));
        assert!(!is_enabled(Path::new("report.xlsx"), &enabled));
        assert!(!is_enabled(Path::new("report.exe"), &enabled));
    }

    #[test]
    fn converts_pptx_with_package_absolute_slide_targets() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("absolute-target.pptx");
        let cursor = Cursor::new(Vec::new());
        let mut archive = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        archive.start_file("ppt/presentation.xml", options).unwrap();
        archive
            .write_all(
                br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#,
            )
            .unwrap();
        archive
            .start_file("ppt/_rels/presentation.xml.rels", options)
            .unwrap();
        archive
            .write_all(
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="/ppt/slides/slide1.xml"/></Relationships>"#,
            )
            .unwrap();
        archive
            .start_file("ppt/slides/slide1.xml", options)
            .unwrap();
        archive
            .write_all(
                br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Absolute slide target</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            )
            .unwrap();
        fs::write(&source, archive.finish().unwrap().into_inner()).unwrap();

        let artifact = convert_locally(&source).unwrap();

        assert!(artifact.markdown.contains("Absolute slide target"));
    }

    #[test]
    #[ignore = "requires CPAHDOCS_PPTX_E2E to point to a local PPTX"]
    fn converts_external_pptx_regression_file() {
        let source = std::env::var("CPAHDOCS_PPTX_E2E")
            .expect("CPAHDOCS_PPTX_E2E must point to a PPTX file");
        let artifact = convert_locally(Path::new(&source)).unwrap();
        eprintln!(
            "converted markdown_bytes={} assets={} warnings={}",
            artifact.markdown.len(),
            artifact.assets.len(),
            artifact.warnings.len()
        );
        assert!(!artifact.markdown.trim().is_empty());
    }

    #[test]
    fn markdown_is_copied_byte_for_byte() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output_root = temporary.path().join("output");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output_root).unwrap();
        let profile = WatchProfile {
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output_root.to_string_lossy().to_string(),
            ..profile()
        };
        let source = input.join("notes.md");
        let output = output_root.join("notes.md");
        let content = b"# Existing markdown\r\n\r\n- keep spacing\r\n";
        fs::write(&source, content).unwrap();

        copy_markdown(&profile, &source, &output).unwrap();

        assert_eq!(fs::read(output).unwrap(), content);
    }

    #[test]
    fn output_uses_original_stem() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output_root = temporary.path().join("output");
        fs::create_dir_all(input.join("reports")).unwrap();
        fs::create_dir_all(&output_root).unwrap();
        let profile = WatchProfile {
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output_root.to_string_lossy().to_string(),
            ..profile()
        };
        let source = input.join("reports").join("same.pdf");
        fs::write(&source, b"pdf").unwrap();
        let output = output_path(&profile, &source).unwrap();
        assert!(output.ends_with(Path::new("reports").join("same.md")));
    }

    #[test]
    fn output_keeps_extensions_when_stems_collide() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output_root = temporary.path().join("output");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output_root).unwrap();
        let profile = WatchProfile {
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output_root.to_string_lossy().to_string(),
            ..profile()
        };
        let pdf = input.join("same.pdf");
        let docx = input.join("same.docx");
        fs::write(&pdf, b"pdf").unwrap();
        fs::write(&docx, b"docx").unwrap();

        assert!(
            output_path(&profile, &pdf)
                .unwrap()
                .ends_with("same.pdf.md")
        );
        assert!(
            output_path(&profile, &docx)
                .unwrap()
                .ends_with("same.docx.md")
        );
    }

    #[test]
    fn markdown_keeps_its_name_when_stems_collide() {
        let temporary = tempfile::tempdir().unwrap();
        let input = temporary.path().join("input");
        let output_root = temporary.path().join("output");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output_root).unwrap();
        let profile = WatchProfile {
            input_dir: input.to_string_lossy().to_string(),
            output_dir: output_root.to_string_lossy().to_string(),
            ..profile()
        };
        let markdown = input.join("same.md");
        let docx = input.join("same.docx");
        fs::write(&markdown, b"# same").unwrap();
        fs::write(&docx, b"docx").unwrap();

        assert!(
            output_path(&profile, &markdown)
                .unwrap()
                .ends_with("same.md")
        );
        assert!(
            output_path(&profile, &docx)
                .unwrap()
                .ends_with("same.docx.md")
        );
    }

    #[test]
    fn rewrites_markdown_and_html_asset_references() {
        let markdown = concat!(
            "![](images/chart%20one.png)\n",
            "<img src=\"images/chart one.png\">\n",
            "[download](<./images/chart one.png>)\n"
        );
        let rewritten = rewrite_asset_reference(
            markdown.to_string(),
            Path::new("images/chart one.png"),
            Path::new("images/chart one.png"),
            "测试 PDF - 副本 (2).assets",
        );

        assert!(rewritten.contains("![](<测试 PDF - 副本 (2).assets/images/chart one.png>)"));
        assert!(
            rewritten.contains("[download](<测试 PDF - 副本 (2).assets/images/chart one.png>)")
        );
        assert!(rewritten.contains(concat!(
            "<img src=\"%E6%B5%8B%E8%AF%95%20PDF%20-%20",
            "%E5%89%AF%E6%9C%AC%20%282%29.assets/images/chart%20one.png\">"
        )));
        assert!(!rewritten.contains("](images/chart%20one.png)"));
        assert!(!rewritten.contains("src=\"images/chart one.png\""));
    }
}
