use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = temporary_sibling(path)?;
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| format!("无法创建临时文件：{}", temporary.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, path)
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn copy_atomic(source: &Path, destination: &Path) -> Result<()> {
    let temporary = temporary_sibling(destination)?;
    let result = (|| -> Result<()> {
        fs::copy(source, &temporary).with_context(|| {
            format!(
                "无法复制 {} 到临时文件 {}",
                source.display(),
                temporary.display()
            )
        })?;
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&temporary)?
            .sync_all()?;
        replace_file(&temporary, destination)
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn replace_file(replacement: &Path, destination: &Path) -> Result<()> {
    replace_file_impl(replacement, destination).with_context(|| {
        format!(
            "无法原子替换 {} -> {}",
            replacement.display(),
            destination.display()
        )
    })
}

fn temporary_sibling(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().context("目标文件没有父目录")?;
    let file_name = path.file_name().context("目标文件名无效")?;
    Ok(parent.join(format!(
        ".{}.{}.cpah.tmp",
        file_name.to_string_lossy(),
        Uuid::new_v4().simple()
    )))
}

#[cfg(windows)]
fn replace_file_impl(replacement: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let replacement_wide = replacement
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            replacement_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error()).context("MoveFileExW 失败");
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file_impl(replacement: &Path, destination: &Path) -> Result<()> {
    fs::rename(replacement, destination).context("rename 失败")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing_content_and_cleans_temporary_file() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("settings.json");
        fs::write(&path, b"old").unwrap();

        write_atomic(&path, b"new").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 1);
    }

    #[test]
    fn atomic_copy_replaces_existing_content() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source.md");
        let destination = temporary.path().join("destination.md");
        fs::write(&source, b"source").unwrap();
        fs::write(&destination, b"old").unwrap();

        copy_atomic(&source, &destination).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"source");
    }
}
