use crate::atomic_file::write_atomic;
use crate::models::WatchProfile;
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde_yaml_ng::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

pub const INDEX_FILE_NAME: &str = "index.md";
const MANAGED_START: &str = "<!-- cpah:index:start -->";
const MANAGED_END: &str = "<!-- cpah:index:end -->";
const MAX_FRONTMATTER_BYTES: u64 = 256 * 1024;
const RECENT_DOCUMENT_LIMIT: usize = 10;

#[derive(Debug, Clone)]
struct IndexedDocument {
    relative_path: PathBuf,
    title: String,
    categories: Vec<String>,
    modified: Option<SystemTime>,
}

pub fn rebuild_profile_index(profile: &WatchProfile) -> Result<()> {
    let root = dunce::canonicalize(&profile.output_dir)
        .with_context(|| format!("无法读取索引输出目录：{}", profile.output_dir))?;
    let documents = discover_documents(&root)?;
    let directories = indexed_directories(&documents);

    let mut ordered = directories.iter().cloned().collect::<Vec<_>>();
    ordered.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in &ordered {
        let managed = render_directory_index(profile, &documents, &directories, directory);
        update_index_file(&root.join(directory).join(INDEX_FILE_NAME), &managed)?;
    }
    clean_stale_indexes(&root, &directories)?;
    Ok(())
}

pub fn is_profile_index(profile: &WatchProfile, path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(INDEX_FILE_NAME))
        && path.starts_with(Path::new(&profile.output_dir))
}

fn discover_documents(root: &Path) -> Result<Vec<IndexedDocument>> {
    let mut documents = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() || !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if is_excluded(path) || !is_markdown(path) {
            continue;
        }
        let relative_path = path
            .strip_prefix(root)
            .context("索引文档不在输出目录内")?
            .to_path_buf();
        let title = relative_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("未命名文档")
            .to_string();
        documents.push(IndexedDocument {
            relative_path,
            title,
            categories: read_categories(path),
            modified: entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok()),
        });
    }
    documents.sort_by_key(|document| document.relative_path.to_string_lossy().to_lowercase());
    Ok(documents)
}

fn indexed_directories(documents: &[IndexedDocument]) -> BTreeSet<PathBuf> {
    let mut directories = BTreeSet::from([PathBuf::new()]);
    for document in documents {
        let mut current = document
            .relative_path
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        loop {
            directories.insert(current.clone());
            if current.as_os_str().is_empty() || !current.pop() {
                break;
            }
        }
    }
    directories
}

fn render_directory_index(
    profile: &WatchProfile,
    documents: &[IndexedDocument],
    directories: &BTreeSet<PathBuf>,
    directory: &Path,
) -> String {
    let subtree = documents
        .iter()
        .enumerate()
        .filter(|(_, document)| path_is_in_directory(&document.relative_path, directory))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let classified = subtree
        .iter()
        .filter(|index| !documents[**index].categories.is_empty())
        .count();
    let pending = subtree.len().saturating_sub(classified);
    let mut output = String::new();
    let title = if directory.as_os_str().is_empty() {
        "知识库索引".to_string()
    } else {
        directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("目录索引")
            .to_string()
    };
    output.push_str(&format!("# {}\n\n", escape_markdown_text(&title)));
    output.push_str(&format!(
        "> {} · 共 {} 篇文档 · 已分类 {} 篇 · 待分类 {} 篇\n",
        escape_markdown_text(&profile.name),
        subtree.len(),
        classified,
        pending,
    ));
    if let Some(modified) = subtree
        .iter()
        .filter_map(|index| documents[*index].modified)
        .max()
    {
        let modified: DateTime<Local> = modified.into();
        output.push_str(&format!(
            "> 最近文档更新：{}\n",
            modified.format("%Y-%m-%d %H:%M")
        ));
    }
    if !directory.as_os_str().is_empty() {
        output.push('\n');
        render_breadcrumbs(&mut output, directory);
    }

    output.push_str("\n## 按文件夹浏览\n\n");
    let child_directories = directories
        .iter()
        .filter(|candidate| {
            !candidate.as_os_str().is_empty() && candidate.parent() == Some(directory)
        })
        .collect::<Vec<_>>();
    let direct_documents = documents
        .iter()
        .enumerate()
        .filter(|(_, document)| {
            document.relative_path.parent().unwrap_or(Path::new("")) == directory
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if child_directories.is_empty() && direct_documents.is_empty() {
        output.push_str("当前目录暂无 Markdown 文档。\n");
    } else {
        for child in child_directories {
            let name = child
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("未命名目录");
            let target = child.join(INDEX_FILE_NAME);
            output.push_str(&format!(
                "- [**{}/**]({})\n",
                escape_link_text(name),
                relative_markdown_link(directory, &target),
            ));
        }
        for index in direct_documents {
            render_document_link(&mut output, directory, &documents[index], "- ");
        }
    }

    output.push_str("\n## 按标签浏览\n\n");
    render_categories(&mut output, profile, directory, &subtree, documents);

    output.push_str("\n## 最近更新\n\n");
    let mut recent = subtree.clone();
    recent.sort_by(|left, right| {
        documents[*right]
            .modified
            .cmp(&documents[*left].modified)
            .then_with(|| {
                documents[*left]
                    .relative_path
                    .cmp(&documents[*right].relative_path)
            })
    });
    if recent.is_empty() {
        output.push_str("暂无 Markdown 文档。\n");
    } else {
        for index in recent.into_iter().take(RECENT_DOCUMENT_LIMIT) {
            render_document_link(&mut output, directory, &documents[index], "- ");
        }
    }
    output
}

fn render_breadcrumbs(output: &mut String, directory: &Path) {
    let mut pieces = vec![format!(
        "[知识库]({})",
        relative_markdown_link(directory, Path::new(INDEX_FILE_NAME))
    )];
    let mut target = PathBuf::new();
    let components = normal_components(directory);
    for (index, component) in components.iter().enumerate() {
        target.push(component);
        if index + 1 == components.len() {
            pieces.push(escape_markdown_text(component));
        } else {
            pieces.push(format!(
                "[{}]({})",
                escape_link_text(component),
                relative_markdown_link(directory, &target.join(INDEX_FILE_NAME)),
            ));
        }
    }
    output.push_str(&format!("> {}\n", pieces.join(" / ")));
}

fn render_categories(
    output: &mut String,
    profile: &WatchProfile,
    directory: &Path,
    indices: &[usize],
    documents: &[IndexedDocument],
) {
    let mut by_category = BTreeMap::<String, Vec<usize>>::new();
    let mut pending = Vec::new();
    for index in indices {
        let document = &documents[*index];
        if document.categories.is_empty() {
            pending.push(*index);
        } else {
            for category in &document.categories {
                by_category
                    .entry(category.clone())
                    .or_default()
                    .push(*index);
            }
        }
    }
    let mut rendered = HashSet::new();
    for label in &profile.tagging.labels {
        if let Some(category_documents) = by_category.get(&label.name) {
            render_category(
                output,
                directory,
                &label.name,
                category_documents,
                documents,
            );
            rendered.insert(label.name.clone());
        }
    }
    for (category, category_documents) in &by_category {
        if rendered.insert(category.clone()) {
            render_category(output, directory, category, category_documents, documents);
        }
    }
    if !pending.is_empty() {
        render_category(output, directory, "待分类", &pending, documents);
    }
    if by_category.is_empty() && pending.is_empty() {
        output.push_str("暂无标签数据。\n");
    }
}

fn render_category(
    output: &mut String,
    directory: &Path,
    category: &str,
    indices: &[usize],
    documents: &[IndexedDocument],
) {
    output.push_str(&format!(
        "### {}（{}）\n\n",
        escape_markdown_text(category),
        indices.len()
    ));
    for index in indices {
        render_document_link(output, directory, &documents[*index], "- ");
    }
    output.push('\n');
}

fn render_document_link(
    output: &mut String,
    directory: &Path,
    document: &IndexedDocument,
    prefix: &str,
) {
    output.push_str(&format!(
        "{}[{}]({})\n",
        prefix,
        escape_link_text(&document.title),
        relative_markdown_link(directory, &document.relative_path),
    ));
}

fn update_index_file(path: &Path, managed: &str) -> Result<()> {
    let current = if path.exists() {
        Some(fs::read(path).with_context(|| format!("无法读取索引：{}", path.display()))?)
    } else {
        None
    };
    let updated = merge_managed_index(current.as_deref(), managed)?;
    if current.as_deref() != Some(updated.as_slice()) {
        write_atomic(path, &updated)?;
    }
    Ok(())
}

fn clean_stale_indexes(root: &Path, desired: &BTreeSet<PathBuf>) -> Result<()> {
    let candidates = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file() && is_index_name(entry.path()))
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    for path in candidates {
        let relative_directory = path
            .parent()
            .and_then(|parent| parent.strip_prefix(root).ok())
            .unwrap_or(Path::new(""));
        if desired.contains(relative_directory) {
            continue;
        }
        let current = fs::read(&path)?;
        let Some(updated) = remove_managed_index(&current)? else {
            continue;
        };
        if content_without_bom(&updated)
            .iter()
            .all(u8::is_ascii_whitespace)
        {
            fs::remove_file(&path)?;
        } else if updated != current {
            write_atomic(&path, &updated)?;
        }
    }
    Ok(())
}

fn read_categories(path: &Path) -> Vec<String> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_FRONTMATTER_BYTES)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Vec::new();
    }
    let bytes = content_without_bom(&bytes);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    let Some(yaml) = frontmatter_yaml(text) else {
        return Vec::new();
    };
    let Ok(Value::Mapping(mapping)) = serde_yaml_ng::from_str::<Value>(yaml) else {
        return Vec::new();
    };
    let Some(Value::Sequence(values)) = mapping.get(Value::String("cpah_categories".to_string()))
    else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    values
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|category| !category.is_empty())
        .filter(|category| seen.insert((*category).to_string()))
        .map(str::to_string)
        .collect()
}

fn frontmatter_yaml(text: &str) -> Option<&str> {
    let opening_end = if text.starts_with("---\r\n") {
        5
    } else if text.starts_with("---\n") {
        4
    } else {
        return None;
    };
    let mut offset = opening_end;
    for segment in text[opening_end..].split_inclusive('\n') {
        let line = segment.trim_end_matches(['\r', '\n']);
        if line == "---" || line == "..." {
            return Some(&text[opening_end..offset]);
        }
        offset += segment.len();
    }
    None
}

fn merge_managed_index(existing: Option<&[u8]>, managed: &str) -> Result<Vec<u8>> {
    let Some(existing) = existing else {
        return Ok(format!(
            "{MANAGED_START}\n{}{MANAGED_END}\n",
            ensure_trailing_newline(managed)
        )
        .into_bytes());
    };
    let (bom, bytes) = split_bom(existing);
    let text = std::str::from_utf8(bytes).context("现有 index.md 不是 UTF-8，已保持原文件不变")?;
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let normalized_managed = managed
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', newline);
    let block = format!(
        "{MANAGED_START}{newline}{}{MANAGED_END}",
        ensure_trailing_newline_with(&normalized_managed, newline)
    );
    let (start, end) = marker_range(text)?;
    let merged = match (start, end) {
        (Some(start), Some(end)) => format!("{}{}{}", &text[..start], block, &text[end..]),
        (None, None) => {
            let separator = if text.is_empty() || text.ends_with(&format!("{newline}{newline}")) {
                String::new()
            } else if text.ends_with(newline) {
                newline.to_string()
            } else {
                format!("{newline}{newline}")
            };
            format!("{text}{separator}{block}{newline}")
        }
        _ => unreachable!(),
    };
    with_bom(bom, merged.as_bytes())
}

fn remove_managed_index(existing: &[u8]) -> Result<Option<Vec<u8>>> {
    let (bom, bytes) = split_bom(existing);
    let text = std::str::from_utf8(bytes).context("现有 index.md 不是 UTF-8，已保持原文件不变")?;
    let (start, end) = marker_range(text)?;
    let (Some(start), Some(end)) = (start, end) else {
        return Ok(None);
    };
    let mut merged = format!("{}{}", &text[..start], &text[end..]);
    while merged.contains("\r\n\r\n\r\n") {
        merged = merged.replace("\r\n\r\n\r\n", "\r\n\r\n");
    }
    while merged.contains("\n\n\n") {
        merged = merged.replace("\n\n\n", "\n\n");
    }
    Ok(Some(with_bom(bom, merged.as_bytes())?))
}

fn marker_range(text: &str) -> Result<(Option<usize>, Option<usize>)> {
    let start = text.find(MANAGED_START);
    let end = start.and_then(|position| {
        text[position + MANAGED_START.len()..]
            .find(MANAGED_END)
            .map(|relative| position + MANAGED_START.len() + relative + MANAGED_END.len())
    });
    match (start, end) {
        (Some(_), None) | (None, Some(_)) => {
            anyhow::bail!("现有 index.md 的 CPAH Docs 托管标记不完整，已保持原文件不变")
        }
        pair => Ok(pair),
    }
}

fn split_bom(bytes: &[u8]) -> (bool, &[u8]) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (true, &bytes[3..])
    } else {
        (false, bytes)
    }
}

fn content_without_bom(bytes: &[u8]) -> &[u8] {
    split_bom(bytes).1
}

fn with_bom(bom: bool, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(bytes.len() + usize::from(bom) * 3);
    if bom {
        output.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    output.extend_from_slice(bytes);
    Ok(output)
}

fn ensure_trailing_newline(value: &str) -> String {
    ensure_trailing_newline_with(value, "\n")
}

fn ensure_trailing_newline_with(value: &str, newline: &str) -> String {
    if value.ends_with(newline) {
        value.to_string()
    } else {
        format!("{value}{newline}")
    }
}

fn path_is_in_directory(path: &Path, directory: &Path) -> bool {
    directory.as_os_str().is_empty() || path.starts_with(directory)
}

fn relative_markdown_link(from_directory: &Path, target: &Path) -> String {
    let from = normal_components(from_directory);
    let to = normal_components(target);
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = PathBuf::new();
    for _ in common..from.len() {
        relative.push("..");
    }
    for component in &to[common..] {
        relative.push(component);
    }
    markdown_link(&relative)
}

fn normal_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            Component::ParentDir => Some("..".to_string()),
            _ => None,
        })
        .collect()
}

fn markdown_link(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::new();
    for character in normalized.chars() {
        if character.is_alphanumeric() || matches!(character, '-' | '_' | '.' | '~' | '/') {
            encoded.push(character);
        } else {
            use std::fmt::Write as _;
            let mut bytes = [0; 4];
            for byte in character.encode_utf8(&mut bytes).bytes() {
                let _ = write!(&mut encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

fn escape_link_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn escape_markdown_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if matches!(
                character,
                '\\' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '`' | '|'
            ) {
                vec!['\\', character]
            } else {
                vec![character]
            }
        })
        .collect()
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn is_index_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(INDEX_FILE_NAME))
}

fn is_excluded(path: &Path) -> bool {
    is_index_name(path)
        || path.components().any(|component| {
            component.as_os_str().to_str().is_some_and(|part| {
                let part = part.to_ascii_lowercase();
                part == ".trash"
                    || part.ends_with(".assets")
                    || part.contains(".cpah.tmp")
                    || part.starts_with("~$")
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CategoryLabel, DeletePolicy, TaggingConfig};

    fn profile(root: &Path) -> WatchProfile {
        WatchProfile {
            id: "index-test".to_string(),
            name: "测试知识库".to_string(),
            input_dir: root.join("input").to_string_lossy().to_string(),
            output_dir: root.join("output").to_string_lossy().to_string(),
            enabled: true,
            delete_policy: DeletePolicy::Keep,
            tagging: TaggingConfig {
                enabled: true,
                selection_mode: Default::default(),
                labels: vec![CategoryLabel {
                    id: "audit".to_string(),
                    name: "审计资料".to_string(),
                    description: String::new(),
                }],
            },
        }
    }

    #[test]
    fn creates_root_and_nested_indexes_with_folder_and_category_views() {
        let temporary = tempfile::tempdir().unwrap();
        let profile = profile(temporary.path());
        let output = Path::new(&profile.output_dir);
        fs::create_dir_all(output.join("项目 A/合同")).unwrap();
        fs::write(
            output.join("项目 A/合同/采购 合同.md"),
            "---\ncpah_categories:\n  - 审计资料\n---\n# 报告\n",
        )
        .unwrap();
        fs::write(output.join("待办.md"), "# 待办\n").unwrap();

        rebuild_profile_index(&profile).unwrap();

        let root = fs::read_to_string(output.join(INDEX_FILE_NAME)).unwrap();
        let nested = fs::read_to_string(output.join("项目 A/合同/index.md")).unwrap();
        assert!(root.contains("## 按文件夹浏览"));
        assert!(root.contains("项目%20A/index.md"));
        assert!(root.contains("### 审计资料（1）"));
        assert!(root.contains("### 待分类（1）"));
        assert!(nested.contains("[知识库](../../index.md)"));
        assert!(nested.contains("采购%20合同.md"));
        assert_eq!(nested.matches(MANAGED_START).count(), 1);
    }

    #[test]
    fn preserves_user_content_bom_and_crlf_when_replacing_managed_block() {
        let existing = format!(
            "\u{feff}# 我的说明\r\n\r\n{MANAGED_START}\r\n旧内容\r\n{MANAGED_END}\r\n\r\n尾注\r\n"
        );
        let merged = merge_managed_index(Some(existing.as_bytes()), "# 新索引\n").unwrap();
        assert!(merged.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = std::str::from_utf8(&merged[3..]).unwrap();
        assert!(text.contains("# 我的说明\r\n"));
        assert!(text.contains("# 新索引\r\n"));
        assert!(text.contains("尾注\r\n"));
        assert!(!text.contains("旧内容"));
    }

    #[test]
    fn source_index_content_is_preserved_but_not_catalogued() {
        let temporary = tempfile::tempdir().unwrap();
        let profile = profile(temporary.path());
        let output = Path::new(&profile.output_dir);
        fs::create_dir_all(output.join("指南")).unwrap();
        fs::write(output.join("指南/index.md"), "# 用户指南\n").unwrap();
        fs::write(output.join("指南/开始.md"), "# 开始\n").unwrap();
        rebuild_profile_index(&profile).unwrap();
        let index = fs::read_to_string(output.join("指南/index.md")).unwrap();
        assert!(index.contains("# 用户指南"));
        assert!(index.contains(MANAGED_START));
        assert!(!index.contains("[index]"));
    }

    #[test]
    fn stale_generated_index_is_removed_and_user_content_survives() {
        let temporary = tempfile::tempdir().unwrap();
        let profile = profile(temporary.path());
        let output = Path::new(&profile.output_dir);
        fs::create_dir_all(output.join("临时")).unwrap();
        fs::write(output.join("临时/文档.md"), "# 文档\n").unwrap();
        rebuild_profile_index(&profile).unwrap();
        fs::remove_file(output.join("临时/文档.md")).unwrap();
        rebuild_profile_index(&profile).unwrap();
        assert!(!output.join("临时/index.md").exists());

        fs::create_dir_all(output.join("说明")).unwrap();
        fs::write(output.join("说明/文档.md"), "# 文档\n").unwrap();
        fs::write(output.join("说明/index.md"), "# 用户内容\n").unwrap();
        rebuild_profile_index(&profile).unwrap();
        fs::remove_file(output.join("说明/文档.md")).unwrap();
        rebuild_profile_index(&profile).unwrap();
        let preserved = fs::read_to_string(output.join("说明/index.md")).unwrap();
        assert!(preserved.contains("# 用户内容"));
        assert!(!preserved.contains(MANAGED_START));
    }

    #[test]
    fn incomplete_markers_leave_existing_index_unchanged() {
        let existing = format!("用户内容\n{MANAGED_START}\n未结束");
        let error = merge_managed_index(Some(existing.as_bytes()), "new").unwrap_err();
        assert!(error.to_string().contains("托管标记不完整"));
    }

    #[test]
    fn generates_large_index_deterministically() {
        let temporary = tempfile::tempdir().unwrap();
        let profile = profile(temporary.path());
        let output = Path::new(&profile.output_dir);
        fs::create_dir_all(output).unwrap();
        for index in 0..5_000 {
            fs::write(output.join(format!("文档-{index:04}.md")), "# test\n").unwrap();
        }
        rebuild_profile_index(&profile).unwrap();
        let first = fs::read(output.join(INDEX_FILE_NAME)).unwrap();
        rebuild_profile_index(&profile).unwrap();
        let second = fs::read(output.join(INDEX_FILE_NAME)).unwrap();
        assert_eq!(first, second);
    }
}
