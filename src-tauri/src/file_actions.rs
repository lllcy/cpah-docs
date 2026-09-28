use crate::converter::{is_enabled, is_supported, output_path};
use crate::file_browser::checked_file;
use crate::models::{AppSettings, JobStatus, WatchProfile};
use crate::storage::Storage;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub fn source_file(
    settings: &AppSettings,
    profile_id: &str,
    relative: &str,
) -> Result<(WatchProfile, PathBuf)> {
    let profile = settings
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .context("找不到已保存的监控目录")?;
    if !profile.enabled {
        bail!("请先启用该监控目录");
    }
    let source = checked_file(Path::new(&profile.input_dir), relative)?;
    if !is_supported(&source) {
        bail!("此文件格式不支持转换");
    }
    if !is_enabled(&source, &settings.enabled_extensions) {
        bail!("请先在格式说明中启用此格式");
    }
    Ok((profile.clone(), source))
}

pub fn classification_output(
    storage: &Storage,
    profile: &WatchProfile,
    source: &Path,
) -> Result<PathBuf> {
    if !profile.tagging.enabled || profile.tagging.labels.is_empty() {
        bail!("请先在目录设置中开启分类并配置候选类别");
    }
    let task = storage
        .find_by_source(&source.to_string_lossy())?
        .context("请先转换此文件")?;
    if task.status != JobStatus::Completed {
        bail!("请先完成此文件的转换");
    }
    let metadata = std::fs::metadata(source)?;
    let modified = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_millis() as i64;
    if task.source_size != Some(metadata.len() as i64) || task.source_modified_ms != Some(modified)
    {
        bail!("源文件已变化，请先重新转换");
    }
    let output = output_path(profile, source)?;
    let relative = output
        .strip_prefix(&profile.output_dir)
        .context("输出文件不在目录范围内")?;
    let output = checked_file(Path::new(&profile.output_dir), &relative.to_string_lossy())
        .context("转换结果不可用，请先重新转换")?;
    if task
        .output_path
        .as_deref()
        .and_then(|path| dunce::canonicalize(path).ok())
        .as_ref()
        != Some(&output)
    {
        bail!("转换输出目录已变化，请先重新转换");
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ConversionEngine;

    #[test]
    fn classification_requires_current_completed_output_and_enabled_rules() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("input");
        let output = root.path().join("output");
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        let input = dunce::canonicalize(input).unwrap();
        let output = dunce::canonicalize(output).unwrap();
        let profile: WatchProfile = serde_json::from_value(serde_json::json!({
            "id": "p", "name": "test", "inputDir": input, "outputDir": output, "enabled": true,
            "deletePolicy": "keep", "tagging": { "enabled": true, "selectionMode": "single", "labels": [{"id":"a", "name":"A", "description":""}] }
        })).unwrap();
        let source = input.join("file.md");
        std::fs::write(&source, "# source").unwrap();
        let store = Storage::new(root.path().join("data")).unwrap();
        let md = output_path(&profile, &source).unwrap();
        let metadata = std::fs::metadata(&source).unwrap();
        let task = store
            .queue_task(
                &profile,
                &source,
                Path::new("file.md"),
                metadata.len(),
                metadata
                    .modified()
                    .unwrap()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
                ConversionEngine::Anytomd,
                &md,
                true,
            )
            .unwrap()
            .unwrap();
        std::fs::write(&md, "# output").unwrap();
        assert!(
            classification_output(&store, &profile, &source)
                .unwrap_err()
                .to_string()
                .contains("完成")
        );
        store
            .set_status(&task.id, JobStatus::Completed, None)
            .unwrap();
        assert_eq!(
            classification_output(&store, &profile, &source).unwrap(),
            md
        );
        let mut disabled = profile.clone();
        disabled.tagging.enabled = false;
        assert!(classification_output(&store, &disabled, &source).is_err());
        std::fs::remove_file(&md).unwrap();
        assert!(
            classification_output(&store, &profile, &source)
                .unwrap_err()
                .to_string()
                .contains("不可用")
        );
        std::fs::write(&md, "# output").unwrap();
        std::fs::write(&source, "# changed source").unwrap();
        assert!(
            classification_output(&store, &profile, &source)
                .unwrap_err()
                .to_string()
                .contains("已变化")
        );
    }
}
