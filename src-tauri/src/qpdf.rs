use crate::state::{ProfileCancelled, ProfileRuntimeControl};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Mutex, OnceLock, TryLockError};
use std::time::{Duration, Instant};

const ARCHIVE: &[u8] = include_bytes!(env!("CPAHDOCS_QPDF_ARCHIVE"));
const TIMEOUT: Duration = Duration::from_secs(300);
static EXECUTABLE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
// Serialize structural PDF work; several 1 GiB inputs must not create several
// QPDF object caches at once. Local extraction still uses the app's scheduler.
static PROCESS_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn check_cancelled(control: Option<&ProfileRuntimeControl>) -> Result<()> {
    if control.is_some_and(ProfileRuntimeControl::is_cancelled) {
        return Err(ProfileCancelled.into());
    }
    Ok(())
}

fn cache_root() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
    base.unwrap_or_else(std::env::temp_dir)
        .join("com.cpah.docs/qpdf")
}

fn hash_reader(mut reader: impl Read) -> Result<Vec<u8>> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 32 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash.finalize().to_vec())
}

fn cache_matches(directory: &Path) -> bool {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(ARCHIVE)) else {
        return false;
    };
    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            return false;
        };
        let Some(relative) = entry.enclosed_name() else {
            return false;
        };
        let Ok(file) = File::open(directory.join(relative)) else {
            return false;
        };
        if !file
            .metadata()
            .is_ok_and(|metadata| metadata.len() == entry.size())
        {
            return false;
        }
        if !matches!((hash_reader(&mut entry), hash_reader(file)), (Ok(expected), Ok(actual)) if expected == actual)
        {
            return false;
        }
    }
    true
}

fn install_runtime(root: &Path) -> Result<PathBuf> {
    fs::create_dir_all(root).context("无法创建 QPDF 运行缓存目录")?;
    let root = dunce::canonicalize(root)?;
    let digest = Sha256::digest(ARCHIVE)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let destination = root.join(digest);
    if !cache_matches(&destination) {
        let staging = tempfile::Builder::new()
            .prefix("install-")
            .tempdir_in(&root)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(ARCHIVE))?;
        let mut total = 0;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            total += entry.size();
            if total > 128 * 1024 * 1024 {
                bail!("内置 QPDF 运行文件大小异常");
            }
            let relative = entry.enclosed_name().context("内置 QPDF 路径不安全")?;
            let path = staging.path().join(&relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = File::create(&path)?;
            std::io::copy(&mut entry, &mut file)?;
            #[cfg(unix)]
            if relative.starts_with("bin") {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
            }
        }
        if destination.exists() {
            // Only replace this payload's own directory inside the cache root.
            let resolved = dunce::canonicalize(&destination)?;
            if resolved == root || !resolved.starts_with(&root) {
                bail!("QPDF 缓存目录路径异常");
            }
            fs::remove_dir_all(&destination)?;
        }
        fs::rename(staging.path(), &destination).context("无法安装内置 QPDF 运行文件")?;
    }
    let name = if cfg!(windows) { "qpdf.exe" } else { "qpdf" };
    Ok(destination.join("bin").join(name))
}

fn executable() -> Result<PathBuf> {
    let mut cached = EXECUTABLE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| anyhow::anyhow!("QPDF 运行缓存锁异常"))?;
    if let Some(path) = cached.as_ref() {
        return Ok(path.clone());
    }
    let path = install_runtime(&cache_root()).context("无法准备内置 QPDF；请重新安装最新版程序")?;
    *cached = Some(path.clone());
    Ok(path)
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Output {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn read_output(file: &mut File) -> Result<String> {
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take(64 * 1024).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

fn run(args: &[OsString], control: Option<&ProfileRuntimeControl>) -> Result<Output> {
    let _permit = loop {
        check_cancelled(control)?;
        match PROCESS_LOCK.try_lock() {
            Ok(permit) => break permit,
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(20)),
            Err(TryLockError::Poisoned(_)) => bail!("QPDF 处理锁异常"),
        }
    };
    let executable = executable()?;
    check_cancelled(control)?;
    let mut command = Command::new(executable);
    command.args(args);
    capture(command, control)
}

fn capture(mut command: Command, control: Option<&ProfileRuntimeControl>) -> Result<Output> {
    check_cancelled(control)?;
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = ChildGuard(command.spawn().context("无法启动内置 QPDF")?);
    let started = Instant::now();
    let status = loop {
        check_cancelled(control)?;
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if started.elapsed() > TIMEOUT {
            bail!("QPDF 处理超时，请检查 PDF 文件后重试");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = Output {
        status,
        stdout: read_output(&mut stdout)?,
        stderr: read_output(&mut stderr)?,
    };
    if status.code() == Some(3) {
        tracing::warn!("QPDF recovered PDF structure with warnings");
    }
    Ok(output)
}

fn require_success(output: &Output) -> Result<()> {
    if !matches!(output.status.code(), Some(0 | 3)) {
        bail!(
            "QPDF 处理失败（退出码 {:?}）：{}",
            output.status.code(),
            output.stderr
        );
    }
    Ok(())
}

pub(crate) fn page_count(source: &Path, control: Option<&ProfileRuntimeControl>) -> Result<u32> {
    let source = dunce::canonicalize(source).context("无法读取 PDF 文件")?;
    let encrypted = run(
        &[source.as_os_str().to_owned(), "--is-encrypted".into()],
        control,
    )?;
    if encrypted.status.code() == Some(0) {
        return Err(anydoc::ConvertError::Encrypted.into());
    }
    let output = run(
        &[source.as_os_str().to_owned(), "--show-npages".into()],
        control,
    )?;
    require_success(&output)?;
    let count = output
        .stdout
        .parse::<u32>()
        .context("QPDF 返回的 PDF 页数无效")?;
    if count == 0 {
        bail!("PDF 不包含可解析页面");
    }
    Ok(count)
}

pub(crate) fn write_page_range(
    source: &Path,
    start: u32,
    end: u32,
    destination: &Path,
    control: Option<&ProfileRuntimeControl>,
) -> Result<u64> {
    let source = dunce::canonicalize(source)?;
    if start == 0 || start > end {
        bail!("PDF 分片页码范围无效：{start}-{end}");
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let destination = std::path::absolute(destination)?;
    let output = run(
        &[
            "--empty".into(),
            "--stream-data=preserve".into(),
            "--pages".into(),
            source.as_os_str().to_owned(),
            format!("{start}-{end}").into(),
            "--".into(),
            destination.as_os_str().to_owned(),
        ],
        control,
    )?;
    require_success(&output)?;
    Ok(fs::metadata(destination)?.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_runtime_installs_repairs_and_reuses_verified_files() {
        let temp = tempfile::tempdir().unwrap();
        let executable = install_runtime(temp.path()).unwrap();
        let output = Command::new(&executable).arg("--version").output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("12.4.2"));
        fs::write(&executable, b"broken runtime").unwrap();
        assert_eq!(install_runtime(temp.path()).unwrap(), executable);
        assert!(
            Command::new(executable)
                .arg("--version")
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn cancelled_qpdf_operation_never_starts_a_child() {
        let control = ProfileRuntimeControl::new();
        control.cancel();
        let error = run(&["--version".into()], Some(&control)).err().unwrap();
        assert!(crate::state::is_profile_cancelled(&error));
    }

    #[test]
    fn in_flight_cancellation_reaps_the_owned_child_promptly() {
        let temporary = tempfile::tempdir().unwrap();
        let marker = temporary.path().join("started");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", "qpdf::tests::waiting_child_probe"])
            .env("CPAHDOCS_QPDF_CHILD_PROBE", &marker);
        let control = ProfileRuntimeControl::new();
        let child_control = control.clone();
        let child = std::thread::spawn(move || capture(command, Some(&child_control)).err());
        let started = Instant::now();
        while !marker.exists() && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let did_start = marker.exists();
        let cancelled = Instant::now();
        control.cancel();
        let error = child.join().unwrap().unwrap();
        assert!(did_start, "child helper did not start");
        assert!(crate::state::is_profile_cancelled(&error));
        // The helper otherwise sleeps for 30 seconds. Returning requires the
        // capture function's guard to kill and wait for its owned process.
        assert!(cancelled.elapsed() < Duration::from_secs(2));
    }

    #[test]
    #[ignore = "owned subprocess helper for cancellation regression"]
    fn waiting_child_probe() {
        if let Some(marker) = std::env::var_os("CPAHDOCS_QPDF_CHILD_PROBE") {
            fs::write(marker, std::process::id().to_string()).unwrap();
            std::thread::sleep(Duration::from_secs(30));
        }
    }
}
