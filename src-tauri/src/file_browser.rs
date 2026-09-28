use crate::converter::{is_enabled, is_supported};
use crate::models::{AppSettings, JobStatus, TaskRecord};
use crate::storage::Storage;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Directory,
    File,
    Link,
    Other,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileAvailability {
    Eligible,
    Disabled,
    Unsupported,
    Link,
    Unreadable,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileFilter {
    #[default]
    All,
    Pending,
    Active,
    Completed,
    Failed,
    Untracked,
    Disabled,
    Unsupported,
    Unreadable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub relative_path: String,
    pub name: String,
    pub kind: FileKind,
    pub size: Option<u64>,
    pub modified_ms: Option<u64>,
    pub availability: FileAvailability,
    pub task: Option<TaskRecord>,
    pub source_changed: bool,
    pub output_available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryRead {
    pub relative_path: String,
    pub error: Option<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBrowserResult {
    pub entries: Vec<FileEntry>,
    pub directories: Vec<DirectoryRead>,
    pub missing_paths: Vec<String>,
    pub matched_count: usize,
}

struct BrowserContext<'a> {
    root: PathBuf,
    output_root: Option<PathBuf>,
    settings: &'a AppSettings,
    tasks: HashMap<String, TaskRecord>,
}

fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

fn is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Junctions must not introduce cycles or escape the configured input root.
        if metadata.is_dir() && metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

fn relative_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Validate each ancestor, including links that resolve back inside the root.
/// Merely checking the final canonical path would still permit link traversal.
fn checked_path(root: &Path, relative: &str, allow_leaf_link: bool) -> Result<PathBuf> {
    let relative = Path::new(relative);
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("无效的目录相对路径");
    }
    let mut path = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        path.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&path)?;
        if is_link(&metadata) {
            if allow_leaf_link && index + 1 == components.len() {
                return Ok(path);
            }
            bail!("链接目录不展开");
        }
        if !dunce::canonicalize(&path)?.starts_with(root) {
            bail!("路径不在所选监控目录中");
        }
    }
    Ok(path)
}

pub(crate) fn checked_file(root: &Path, relative: &str) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(root).context("无法读取文件目录")?;
    if !metadata.is_dir() || is_link(&metadata) {
        bail!("文件目录必须是普通文件夹");
    }
    let root = dunce::canonicalize(root)?;
    let path = checked_path(&root, relative, false)?;
    if !path.is_file() {
        bail!("请选择普通文件");
    }
    Ok(dunce::canonicalize(path)?)
}

impl<'a> BrowserContext<'a> {
    fn new(storage: &Storage, settings: &'a AppSettings, profile_id: &str) -> Result<Self> {
        let profile = settings
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .context("找不到已保存的监控目录")?;
        let metadata = fs::symlink_metadata(&profile.input_dir)
            .context("无法读取监控目录，请检查目录是否存在以及访问权限")?;
        if !metadata.is_dir() || is_link(&metadata) {
            bail!("监控目录必须是普通文件夹，不能是链接");
        }
        let root = dunce::canonicalize(&profile.input_dir)?;
        // This query intentionally has no dashboard history limit and no PDF parts.
        let tags = storage
            .list_profile_tag_jobs(profile_id)?
            .into_iter()
            .map(|job| (path_key(Path::new(&job.markdown_path)), job))
            .collect::<HashMap<_, _>>();
        let tasks = storage
            .list_profile_tasks(profile_id)?
            .into_iter()
            .map(|mut task| {
                if let Some(job) = task
                    .output_path
                    .as_deref()
                    .and_then(|output| tags.get(&path_key(Path::new(output))))
                {
                    task.tag_job_id = Some(job.id.clone());
                    task.tag_status = Some(job.status.clone());
                }
                (path_key(Path::new(&task.source_path)), task)
            })
            .collect();
        Ok(Self {
            root,
            output_root: dunce::canonicalize(&profile.output_dir).ok(),
            settings,
            tasks,
        })
    }

    fn entry(&self, path: &Path) -> FileEntry {
        let relative_path = relative_key(path.strip_prefix(&self.root).unwrap_or(path));
        let mut entry = FileEntry {
            relative_path,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            kind: FileKind::Other,
            size: None,
            modified_ms: None,
            availability: FileAvailability::Unreadable,
            task: None,
            source_changed: false,
            output_available: false,
            error: None,
        };
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                entry.error = Some(format!("无法读取文件信息：{error}"));
                return entry;
            }
        };
        if is_link(&metadata) {
            entry.kind = FileKind::Link;
            entry.availability = FileAvailability::Link;
            return entry;
        }
        entry.kind = if metadata.is_dir() {
            FileKind::Directory
        } else if metadata.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        };
        entry.modified_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64);
        if entry.kind == FileKind::Directory {
            return entry;
        }
        entry.size = Some(metadata.len());
        entry.availability = if !metadata.is_file() || !is_supported(path) {
            FileAvailability::Unsupported
        } else if !is_enabled(path, &self.settings.enabled_extensions) {
            FileAvailability::Disabled
        } else {
            FileAvailability::Eligible
        };
        entry.task = self.tasks.get(&path_key(path)).cloned();
        if let Some(task) = &entry.task {
            entry.source_changed = task.status == JobStatus::Completed
                && (task
                    .source_size
                    .is_some_and(|size| size as u64 != metadata.len())
                    || task.source_modified_ms.is_some_and(|modified| {
                        entry
                            .modified_ms
                            .is_some_and(|current| current != modified as u64)
                    }));
            entry.output_available = task.output_path.as_ref().is_some_and(|output| {
                dunce::canonicalize(output).ok().is_some_and(|resolved| {
                    self.output_root
                        .as_ref()
                        .is_some_and(|root| resolved.starts_with(root))
                        && resolved.is_file()
                })
            });
        }
        entry
    }
}

fn matches_filter(entry: &FileEntry, filter: FileFilter) -> bool {
    if entry.kind == FileKind::Directory {
        return false;
    }
    if filter == FileFilter::All {
        return true;
    }
    if filter == FileFilter::Unreadable {
        return entry.availability == FileAvailability::Unreadable;
    }
    match entry.task.as_ref().map(|task| &task.status) {
        Some(status) => match filter {
            FileFilter::Pending => matches!(
                status,
                JobStatus::WaitingStable | JobStatus::Queued | JobStatus::WaitingMineru
            ),
            FileFilter::Active => matches!(
                status,
                JobStatus::Converting
                    | JobStatus::WaitingParts
                    | JobStatus::Uploading
                    | JobStatus::Processing
                    | JobStatus::Downloading
            ),
            FileFilter::Completed => *status == JobStatus::Completed,
            FileFilter::Failed => *status == JobStatus::Failed,
            _ => false,
        },
        None => matches!(
            (filter, &entry.availability),
            (FileFilter::Untracked, FileAvailability::Eligible)
                | (FileFilter::Disabled, FileAvailability::Disabled)
                | (FileFilter::Unsupported, FileAvailability::Unsupported)
        ),
    }
}

pub fn list_files(
    storage: &Storage,
    settings: &AppSettings,
    profile_id: &str,
    directories: Vec<String>,
    files: Vec<String>,
) -> Result<FileBrowserResult> {
    let context = BrowserContext::new(storage, settings, profile_id)?;
    let mut result = FileBrowserResult::default();
    let mut entries = HashMap::new();
    for relative in directories.into_iter().collect::<HashSet<_>>() {
        let read = (|| -> Result<()> {
            let path = checked_path(&context.root, &relative, false)?;
            for child in fs::read_dir(path)? {
                let child = child?;
                let entry = context.entry(&child.path());
                entries.insert(entry.relative_path.clone(), entry);
            }
            Ok(())
        })();
        result.directories.push(DirectoryRead {
            relative_path: relative,
            error: read.err().map(|error| format!("无法展开文件夹：{error}")),
        });
    }
    for relative in files.into_iter().collect::<HashSet<_>>() {
        match checked_path(&context.root, &relative, true) {
            Ok(path) => {
                let entry = context.entry(&path);
                entries.insert(entry.relative_path.clone(), entry);
            }
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                result.missing_paths.push(relative)
            }
            Err(error) => {
                // Return a visible error, never stale success, for an inaccessible item.
                if !Path::new(&relative)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                {
                    return Err(error);
                }
                entries.insert(
                    relative.clone(),
                    FileEntry {
                        name: Path::new(&relative)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into(),
                        relative_path: relative,
                        kind: FileKind::Other,
                        size: None,
                        modified_ms: None,
                        availability: FileAvailability::Unreadable,
                        task: None,
                        source_changed: false,
                        output_available: false,
                        error: Some(error.to_string()),
                    },
                );
            }
        }
    }
    result.entries = entries.into_values().collect();
    Ok(result)
}

pub fn search_files(
    storage: &Storage,
    settings: &AppSettings,
    profile_id: &str,
    query: &str,
    filter: FileFilter,
) -> Result<FileBrowserResult> {
    let context = BrowserContext::new(storage, settings, profile_id)?;
    let query = query.trim().to_lowercase();
    let mut result = FileBrowserResult::default();
    let mut folders = HashMap::new();
    let mut required_folders = HashSet::new();
    let mut walker = WalkDir::new(&context.root).follow_links(false).into_iter();
    while let Some(item) = walker.next() {
        let item = match item {
            Ok(item) => item,
            Err(error) => {
                let path = error.path().unwrap_or(&context.root);
                let relative =
                    relative_key(path.strip_prefix(&context.root).unwrap_or(Path::new("")));
                result.directories.push(DirectoryRead {
                    relative_path: relative.clone(),
                    error: Some(format!("无法读取：{error}")),
                });
                required_folders.insert(relative);
                continue;
            }
        };
        if item.depth() == 0 {
            continue;
        }
        let entry = context.entry(item.path());
        if entry.kind == FileKind::Link && item.file_type().is_dir() {
            walker.skip_current_dir();
        }
        if entry.kind == FileKind::Directory {
            folders.insert(entry.relative_path.clone(), entry);
        } else if entry.name.to_lowercase().contains(&query) && matches_filter(&entry, filter) {
            if let Some(parent) = Path::new(&entry.relative_path).parent() {
                required_folders.insert(relative_key(parent));
            }
            result.matched_count += 1;
            result.entries.push(entry);
        }
    }
    let mut ancestors = HashSet::new();
    for relative in required_folders {
        for parent in Path::new(&relative)
            .ancestors()
            .filter(|path| !path.as_os_str().is_empty())
        {
            ancestors.insert(relative_key(parent));
        }
    }
    for relative in ancestors {
        if let Some(folder) = folders.remove(&relative) {
            result.entries.push(folder);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ConversionEngine, DeletePolicy, TaggingConfig, WatchProfile};

    struct Fixture {
        temp: tempfile::TempDir,
        storage: Storage,
        settings: AppSettings,
        input: PathBuf,
        output: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let input = temp.path().join("input");
            let output = temp.path().join("output");
            fs::create_dir_all(&input).unwrap();
            fs::create_dir_all(&output).unwrap();
            let input = dunce::canonicalize(input).unwrap();
            let output = dunce::canonicalize(output).unwrap();
            let storage = Storage::new(temp.path().join("data")).unwrap();
            let mut settings = AppSettings::default();
            settings.profiles.push(WatchProfile {
                id: "files".into(),
                name: "文件".into(),
                input_dir: input.to_string_lossy().into(),
                output_dir: output.to_string_lossy().into(),
                enabled: false,
                delete_policy: DeletePolicy::Keep,
                tagging: TaggingConfig::default(),
            });
            settings.monitoring_paused = true;
            Self {
                temp,
                storage,
                settings,
                input,
                output,
            }
        }

        fn write(&self, relative: &str) -> PathBuf {
            let path = self.input.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "test document").unwrap();
            path
        }

        fn task(&self, relative: &str, status: JobStatus) -> TaskRecord {
            let path = self.write(relative);
            let metadata = fs::metadata(&path).unwrap();
            let modified = metadata
                .modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let task = self
                .storage
                .queue_task(
                    &self.settings.profiles[0],
                    &path,
                    Path::new(relative),
                    metadata.len(),
                    modified,
                    ConversionEngine::Anytomd,
                    &self.output.join(relative).with_extension("md"),
                    false,
                )
                .unwrap()
                .unwrap();
            self.storage.set_status(&task.id, status, None).unwrap();
            self.storage.get_task(&task.id).unwrap().unwrap()
        }

        fn list(&self, directories: &[&str], files: &[&str]) -> FileBrowserResult {
            list_files(
                &self.storage,
                &self.settings,
                "files",
                directories.iter().map(|s| (*s).into()).collect(),
                files.iter().map(|s| (*s).into()).collect(),
            )
            .unwrap()
        }
    }

    #[test]
    fn browsing_all_file_types_and_empty_folders_has_no_queue_or_output_side_effects() {
        let mut fixture = Fixture::new();
        fixture
            .settings
            .enabled_extensions
            .retain(|extension| extension != "jpg");
        fixture.write("document.txt");
        fixture.write("photo.jpg");
        fixture.write("archive.zip");
        fixture.write(".hidden.txt");
        fixture.write("nested/child.txt");
        fs::create_dir(fixture.input.join("empty")).unwrap();
        let result = fixture.list(&[""], &[]);
        assert_eq!(result.entries.len(), 6);
        assert!(
            !result
                .entries
                .iter()
                .any(|entry| entry.relative_path.contains('/'))
        );
        assert_eq!(
            result
                .entries
                .iter()
                .find(|entry| entry.name == "photo.jpg")
                .unwrap()
                .availability,
            FileAvailability::Disabled
        );
        assert_eq!(
            result
                .entries
                .iter()
                .find(|entry| entry.name == "archive.zip")
                .unwrap()
                .availability,
            FileAvailability::Unsupported
        );
        assert_eq!(fixture.storage.task_count().unwrap(), 0);
        assert_eq!(fs::read_dir(&fixture.output).unwrap().count(), 0);
        assert!(fixture.list(&["empty"], &[]).entries.is_empty());
    }

    #[test]
    fn recursive_search_keeps_ancestors_and_uses_actual_task_status() {
        let fixture = Fixture::new();
        fixture.task("2026/contracts/合同.txt", JobStatus::Failed);
        fixture.task("2026/contracts/other.txt", JobStatus::Completed);
        fixture.write("new.txt");
        let result = search_files(
            &fixture.storage,
            &fixture.settings,
            "files",
            "合同",
            FileFilter::Failed,
        )
        .unwrap();
        assert_eq!(result.matched_count, 1);
        let paths = result
            .entries
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(
            paths,
            HashSet::from(["2026", "2026/contracts", "2026/contracts/合同.txt"])
        );
        let new = search_files(
            &fixture.storage,
            &fixture.settings,
            "files",
            "",
            FileFilter::Untracked,
        )
        .unwrap();
        assert_eq!(new.matched_count, 1);
        assert_eq!(new.entries[0].name, "new.txt");
    }

    #[test]
    fn finished_files_report_changes_missing_outputs_and_disappearing_sources() {
        let fixture = Fixture::new();
        let task = fixture.task("note.txt", JobStatus::Completed);
        fs::write(task.output_path.as_ref().unwrap(), "output").unwrap();
        let result = fixture.list(&[], &["note.txt"]);
        assert!(!result.entries[0].source_changed);
        assert!(result.entries[0].output_available);
        fs::write(fixture.input.join("note.txt"), "changed source document").unwrap();
        fs::remove_file(task.output_path.as_ref().unwrap()).unwrap();
        let result = fixture.list(&[], &["note.txt"]);
        assert!(result.entries[0].source_changed);
        assert!(!result.entries[0].output_available);
        fs::remove_file(fixture.input.join("note.txt")).unwrap();
        assert_eq!(
            fixture.list(&[], &["note.txt"]).missing_paths,
            vec!["note.txt"]
        );
    }

    #[test]
    fn invalid_or_unreadable_subdirectory_does_not_hide_other_entries() {
        let fixture = Fixture::new();
        fixture.write("good.txt");
        let result = fixture.list(&["", "missing", "good.txt", "../output"], &[]);
        assert_eq!(result.entries.len(), 1);
        assert_eq!(
            result
                .directories
                .iter()
                .filter(|directory| directory.error.is_some())
                .count(),
            3
        );
        assert!(
            list_files(
                &fixture.storage,
                &fixture.settings,
                "unknown",
                vec!["".into()],
                vec![]
            )
            .is_err()
        );
        assert!(
            list_files(
                &fixture.storage,
                &fixture.settings,
                "files",
                vec![],
                vec!["../output".into()]
            )
            .is_err()
        );
        assert!(checked_path(&fixture.input, &fixture.output.to_string_lossy(), false).is_err());
    }

    #[test]
    fn old_tasks_are_visible_beyond_dashboard_limit() {
        let fixture = Fixture::new();
        let task = fixture.task("old.txt", JobStatus::Completed);
        let mut connection =
            rusqlite::Connection::open(fixture.temp.path().join("data/converter.db")).unwrap();
        let transaction = connection.transaction().unwrap();
        for index in 0..5001 {
            transaction.execute("INSERT INTO tasks (id,profile_id,source_path,relative_path,engine,status,created_at,updated_at) VALUES (?1,'files',?1,?1,'anytomd','queued','2099','2099')", [format!("new-{index}")]).unwrap();
        }
        transaction.commit().unwrap();
        assert!(
            !fixture
                .storage
                .list_visible_tasks(5000)
                .unwrap()
                .iter()
                .any(|item| item.id == task.id)
        );
        let result = fixture.list(&[""], &[]);
        assert_eq!(result.entries[0].task.as_ref().unwrap().id, task.id);
        assert_eq!(
            result.entries[0].task.as_ref().unwrap().status,
            JobStatus::Completed
        );
    }

    #[test]
    fn directory_links_are_visible_without_following_or_reading_their_targets() {
        let fixture = Fixture::new();
        fs::write(fixture.output.join("private.txt"), "outside").unwrap();
        fixture.write("visible.txt");
        let link = fixture.input.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&fixture.output, &link).unwrap();
        #[cfg(windows)]
        {
            // Directory junctions exercise Windows reparse handling without admin rights.
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&fixture.output)
                .output()
                .unwrap();
            assert!(
                status.status.success(),
                "{}",
                String::from_utf8_lossy(&status.stderr)
            );
        }
        let result = fixture.list(&[""], &[]);
        assert_eq!(
            result
                .entries
                .iter()
                .find(|entry| entry.name == "link")
                .unwrap()
                .kind,
            FileKind::Link
        );
        assert!(fixture.list(&["link"], &[]).directories[0].error.is_some());
        let refused = fixture.list(&[], &["link/private.txt"]);
        assert_eq!(refused.entries[0].size, None);
        assert!(refused.entries[0].error.is_some());
        let search = search_files(
            &fixture.storage,
            &fixture.settings,
            "files",
            "",
            FileFilter::All,
        )
        .unwrap();
        assert!(search.entries.iter().any(|entry| entry.name == "link"));
        assert!(
            search
                .entries
                .iter()
                .any(|entry| entry.name == "visible.txt")
        );
        assert!(
            !search
                .entries
                .iter()
                .any(|entry| entry.name == "private.txt")
        );
    }
}
