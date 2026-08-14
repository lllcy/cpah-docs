use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const LOG_BACKUPS: usize = 3;

#[derive(Clone)]
struct RotatingMakeWriter {
    inner: Arc<Mutex<RotatingFile>>,
}

struct RotatingFile {
    directory: PathBuf,
    file: Option<File>,
    bytes: u64,
}

struct RotatingWriter {
    inner: Arc<Mutex<RotatingFile>>,
}

pub fn initialize(data_dir: &Path) -> Result<()> {
    let directory = data_dir.join("logs");
    fs::create_dir_all(&directory)
        .with_context(|| format!("无法创建日志目录：{}", directory.display()))?;
    let path = directory.join("cpah-docs.log");
    let file = open_log(&path)?;
    let bytes = file
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or_default();
    let writer = RotatingMakeWriter {
        inner: Arc::new(Mutex::new(RotatingFile {
            directory,
            file: Some(file),
            bytes,
        })),
    };
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_writer(writer)
        .try_init()
        .map_err(|error| anyhow::anyhow!("无法初始化应用日志：{error}"))?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "CPAH Docs started");
    Ok(())
}

impl<'a> MakeWriter<'a> for RotatingMakeWriter {
    type Writer = RotatingWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RotatingWriter {
            inner: self.inner.clone(),
        }
    }
}

impl Write for RotatingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("log lock poisoned"))?;
        if inner.bytes.saturating_add(buffer.len() as u64) > MAX_LOG_BYTES {
            inner.rotate()?;
        }
        let written = inner
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file unavailable"))?
            .write(buffer)?;
        inner.bytes = inner.bytes.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("log lock poisoned"))?;
        inner
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file unavailable"))?
            .flush()
    }
}

impl RotatingFile {
    fn rotate(&mut self) -> io::Result<()> {
        self.file.take();
        for number in (1..=LOG_BACKUPS).rev() {
            let source = if number == 1 {
                self.directory.join("cpah-docs.log")
            } else {
                self.directory.join(format!("cpah-docs.log.{}", number - 1))
            };
            let target = self.directory.join(format!("cpah-docs.log.{number}"));
            if number == LOG_BACKUPS && target.exists() {
                fs::remove_file(&target)?;
            }
            if source.exists() {
                fs::rename(source, target)?;
            }
        }
        self.file = Some(open_log(&self.directory.join("cpah-docs.log"))?);
        self.bytes = 0;
        Ok(())
    }
}

fn open_log(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_only_inside_the_configured_log_directory() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("logs");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("cpah-docs.log"), b"current").unwrap();
        let file = open_log(&directory.join("cpah-docs.log")).unwrap();
        let mut rotating = RotatingFile {
            directory: directory.clone(),
            file: Some(file),
            bytes: 7,
        };
        rotating.rotate().unwrap();
        assert_eq!(
            fs::read(directory.join("cpah-docs.log.1")).unwrap(),
            b"current"
        );
        assert!(directory.join("cpah-docs.log").exists());
    }
}
