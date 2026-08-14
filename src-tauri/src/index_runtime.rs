use crate::knowledge_index::{is_profile_index, rebuild_profile_index};
use crate::models::WatchProfile;
use crate::state::AppState;
use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const DEBOUNCE: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub enum IndexRuntimeMessage {
    Reload,
    Path(PathBuf),
}

pub fn start(state: AppState) -> Result<()> {
    let (sender, receiver) = mpsc::unbounded_channel();
    state.set_index_runtime_sender(sender.clone());
    let health = state.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run(state, sender, receiver).await {
            let message = format!("知识库索引后台已停止：{error:#}");
            tracing::error!("{message}");
            health.set_index_runtime_error(message);
        }
    });
    Ok(())
}

async fn run(
    state: AppState,
    sender: mpsc::UnboundedSender<IndexRuntimeMessage>,
    mut receiver: mpsc::UnboundedReceiver<IndexRuntimeMessage>,
) -> Result<()> {
    let event_sender = sender.clone();
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<Event>| match result {
            Ok(event) => {
                for path in event.paths {
                    let _ = event_sender.send(IndexRuntimeMessage::Path(path));
                }
            }
            Err(error) => tracing::error!(error = %error, "index output watcher error"),
        })?;
    let mut watched_roots = Vec::new();
    reload_watches(&state, &mut watcher, &mut watched_roots).await?;
    rebuild_all(&state).await;

    let mut dirty = HashMap::<String, Instant>::new();
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    interval.tick().await;
    loop {
        tokio::select! {
            Some(message) = receiver.recv() => {
                match message {
                    IndexRuntimeMessage::Reload => {
                        if let Err(error) = reload_watches(&state, &mut watcher, &mut watched_roots).await {
                            tracing::error!(error = %format!("{error:#}"), "index watcher reload failed");
                        }
                        let settings = state.settings.read().await.clone();
                        for profile in settings.profiles.into_iter().filter(|profile| profile.enabled) {
                            dirty.insert(profile.id, Instant::now() - DEBOUNCE);
                        }
                    }
                    IndexRuntimeMessage::Path(path) => {
                        if let Some(profile_id) = affected_profile(&state, &path).await {
                            dirty.insert(profile_id, Instant::now());
                        }
                    }
                }
            }
            _ = interval.tick() => {
                let now = Instant::now();
                let due = dirty
                    .iter()
                    .filter(|(_, changed)| now.duration_since(**changed) >= DEBOUNCE)
                    .map(|(profile_id, _)| profile_id.clone())
                    .collect::<Vec<_>>();
                for profile_id in due {
                    dirty.remove(&profile_id);
                    rebuild_one(&state, &profile_id).await;
                }
            }
            else => break,
        }
    }
    Ok(())
}

async fn reload_watches(
    state: &AppState,
    watcher: &mut RecommendedWatcher,
    watched_roots: &mut Vec<PathBuf>,
) -> Result<()> {
    for root in watched_roots.drain(..) {
        let _ = watcher.unwatch(&root);
    }
    let settings = state.settings.read().await.clone();
    for profile in settings
        .profiles
        .into_iter()
        .filter(|profile| profile.enabled)
    {
        let root = PathBuf::from(profile.output_dir);
        if root.is_dir() {
            watcher.watch(&root, RecursiveMode::Recursive)?;
            watched_roots.push(root);
        }
    }
    Ok(())
}

async fn affected_profile(state: &AppState, path: &Path) -> Option<String> {
    if path.components().any(|component| {
        component.as_os_str().to_str().is_some_and(|part| {
            let part = part.to_ascii_lowercase();
            part == ".trash" || part.ends_with(".assets") || part.contains(".cpah.tmp")
        })
    }) {
        return None;
    }
    let settings = state.settings.read().await;
    settings
        .profiles
        .iter()
        .filter(|profile| profile.enabled)
        .filter(|profile| path.starts_with(&profile.output_dir))
        .filter(|profile| !is_profile_index(profile, path))
        .max_by_key(|profile| Path::new(&profile.output_dir).components().count())
        .map(|profile| profile.id.clone())
}

async fn rebuild_all(state: &AppState) {
    let profiles = state
        .settings
        .read()
        .await
        .profiles
        .iter()
        .filter(|profile| profile.enabled)
        .cloned()
        .collect::<Vec<_>>();
    for profile in profiles {
        rebuild(state, profile).await;
    }
}

async fn rebuild_one(state: &AppState, profile_id: &str) {
    let profile = state
        .settings
        .read()
        .await
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id && profile.enabled)
        .cloned();
    if let Some(profile) = profile {
        rebuild(state, profile).await;
    }
}

async fn rebuild(state: &AppState, profile: WatchProfile) {
    let profile_name = profile.name.clone();
    match tokio::task::spawn_blocking(move || rebuild_profile_index(&profile)).await {
        Ok(Ok(())) => state.clear_index_runtime_error(),
        Ok(Err(error)) => {
            let message = format!("知识库索引更新失败（{profile_name}）：{error:#}");
            state.set_index_runtime_error(message.clone());
            tracing::error!(profile = %profile_name, error = %format!("{error:#}"), "index rebuild failed");
        }
        Err(error) => {
            let message = format!("知识库索引任务异常（{profile_name}）：{error}");
            state.set_index_runtime_error(message);
            tracing::error!(profile = %profile_name, error = %error, "index rebuild task failed");
        }
    }
}
