use super::*;

async fn fixture() -> (tempfile::TempDir, AppState, WatchProfile, WatchProfile) {
    let root = tempfile::tempdir().unwrap();
    let mut profiles = Vec::new();
    for label in ["A", "B"] {
        let input = root.path().join(label);
        let output = root.path().join(format!("{label}-output"));
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        profiles.push(WatchProfile {
            id: label.into(),
            name: label.into(),
            input_dir: dunce::canonicalize(input)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            output_dir: dunce::canonicalize(output)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            enabled: true,
            delete_policy: DeletePolicy::Keep,
            tagging: Default::default(),
        });
    }
    let state = AppState::new(root.path().join("data")).unwrap();
    {
        let mut settings = state.settings.write().await;
        settings.profiles = profiles.clone();
        settings.enabled_extensions = vec!["md".into()];
        settings.paused = true;
        settings.monitoring_paused = false;
        state.storage.save_settings(&settings).unwrap();
    }
    state.ensure_profile_controls(["A", "B"]);
    state.set_paused_flag(true);
    state.set_monitoring_paused_flag(false);
    let b = profiles.pop().unwrap();
    let a = profiles.pop().unwrap();
    (root, state, a, b)
}

async fn scan(state: &AppState) {
    scan_all(
        state,
        &MinerUClient::new().unwrap(),
        &Arc::new(Mutex::new(ActivePaths::default())),
        &Arc::new(Semaphore::new(2)),
    )
    .await
    .unwrap();
}

async fn wait_for_task(state: &AppState, source: &Path) -> TaskRecord {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Some(task) = state
                .storage
                .find_by_source(&source.to_string_lossy())
                .unwrap()
            {
                return task;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("file was not discovered: {}", source.display()))
}

struct RunningMonitor(tokio::task::JoinHandle<Result<()>>);

impl RunningMonitor {
    fn start(state: &AppState) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        state.set_runtime_sender(sender.clone());
        Self(tokio::spawn(run(state.clone(), sender, receiver)))
    }

    async fn stop(mut self) {
        self.0.abort();
        assert!((&mut self.0).await.unwrap_err().is_cancelled());
    }
}

impl Drop for RunningMonitor {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Default)]
struct RecordingWatcher {
    watched: HashSet<PathBuf>,
    watch_calls: Vec<PathBuf>,
    unwatch_calls: Vec<PathBuf>,
    rejected: Option<PathBuf>,
    rejected_removal: Option<PathBuf>,
}

impl Watcher for RecordingWatcher {
    fn new<F: notify::EventHandler>(_handler: F, _config: notify::Config) -> notify::Result<Self> {
        Ok(Self::default())
    }

    fn watch(&mut self, path: &Path, _mode: RecursiveMode) -> notify::Result<()> {
        self.watch_calls.push(path.to_path_buf());
        if self.rejected.as_deref() == Some(path) {
            return Err(notify::Error::generic("simulated registration failure")
                .add_path(path.to_path_buf()));
        }
        self.watched.insert(path.to_path_buf());
        Ok(())
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        self.unwatch_calls.push(path.to_path_buf());
        if self.rejected_removal.as_deref() == Some(path) {
            return Err(
                notify::Error::generic("simulated removal failure").add_path(path.to_path_buf())
            );
        }
        self.watched.remove(path);
        Ok(())
    }

    fn kind() -> notify::WatcherKind {
        notify::WatcherKind::NullWatcher
    }
}

#[tokio::test]
async fn missing_source_does_not_block_other_profiles_in_either_order() {
    let (root, state, a, b) = fixture().await;
    std::fs::rename(&a.input_dir, root.path().join("A-relocated")).unwrap();
    for filename in ["a-first.md", "b-first.md"] {
        let source = Path::new(&b.input_dir).join(filename);
        std::fs::write(&source, "# B survives a missing A").unwrap();
        scan(&state).await;
        let task = wait_for_task(&state, &source).await;
        assert_eq!(task.profile_id, b.id);
        assert_eq!(task.status, JobStatus::Queued);
        assert_eq!(
            task.output_path,
            Some(
                Path::new(&b.output_dir)
                    .join(filename)
                    .to_string_lossy()
                    .into_owned()
            )
        );
        state.settings.write().await.profiles.reverse();
    }
}

#[tokio::test]
async fn unusable_output_does_not_block_other_profiles() {
    let (root, state, a, b) = fixture().await;
    std::fs::rename(&a.output_dir, root.path().join("A-output-relocated")).unwrap();
    std::fs::write(&a.output_dir, "not a directory").unwrap();
    let source = Path::new(&b.input_dir).join("healthy.md");
    std::fs::write(&source, "# healthy B").unwrap();
    scan(&state).await;
    assert_eq!(wait_for_task(&state, &source).await.profile_id, b.id);
}

#[tokio::test]
async fn watch_registration_failure_does_not_block_other_profiles_or_retries() {
    let (_root, state, a, b) = fixture().await;
    let mut watcher = RecordingWatcher {
        rejected: Some(PathBuf::from(&a.input_dir)),
        ..Default::default()
    };
    let mut roots = Vec::new();
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    assert_eq!(roots, vec![PathBuf::from(&b.input_dir)]);
    assert_eq!(
        watcher.watched,
        HashSet::from([PathBuf::from(&b.input_dir)])
    );

    watcher.rejected = None;
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    assert_eq!(
        watcher.watched,
        HashSet::from([PathBuf::from(&a.input_dir), PathBuf::from(&b.input_dir)])
    );
    assert_eq!(
        watcher
            .watch_calls
            .iter()
            .filter(|path| path.as_path() == Path::new(&b.input_dir))
            .count(),
        1
    );
    assert!(watcher.unwatch_calls.is_empty());
}

#[tokio::test]
async fn changing_one_source_preserves_the_other_watch_even_if_new_registration_fails() {
    let (root, state, a, b) = fixture().await;
    let mut watcher = RecordingWatcher::default();
    let mut roots = Vec::new();
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    assert_eq!(watcher.watch_calls.len(), 2);
    assert!(watcher.unwatch_calls.is_empty());

    let relocated = root.path().join("A-relocated");
    std::fs::rename(&a.input_dir, &relocated).unwrap();
    let relocated = dunce::canonicalize(relocated).unwrap();
    state.settings.write().await.profiles[0].input_dir = relocated.to_string_lossy().into_owned();
    watcher.rejected = Some(relocated.clone());
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    assert_eq!(roots, vec![PathBuf::from(&b.input_dir)]);
    assert_eq!(watcher.unwatch_calls, vec![PathBuf::from(&a.input_dir)]);

    watcher.rejected = None;
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    assert!(watcher.watched.contains(&relocated));
    assert_eq!(
        watcher
            .watch_calls
            .iter()
            .filter(|path| path.as_path() == Path::new(&b.input_dir))
            .count(),
        1
    );
}

#[tokio::test]
async fn failed_watch_removal_is_retried_without_interrupting_other_profiles() {
    let (_root, state, a, b) = fixture().await;
    let mut watcher = RecordingWatcher::default();
    let mut roots = Vec::new();
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    state.settings.write().await.profiles[0].enabled = false;
    watcher.rejected_removal = Some(PathBuf::from(&a.input_dir));
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    assert!(roots.contains(&PathBuf::from(&a.input_dir)));
    assert!(watcher.watched.contains(&PathBuf::from(&b.input_dir)));

    watcher.rejected_removal = None;
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    assert_eq!(roots, vec![PathBuf::from(&b.input_dir)]);
    assert_eq!(watcher.unwatch_calls, vec![PathBuf::from(&a.input_dir); 2]);
}

#[tokio::test]
async fn disabling_a_profile_and_stopping_monitoring_remove_their_watches() {
    let (_root, state, a, b) = fixture().await;
    let mut watcher = RecordingWatcher::default();
    let mut roots = Vec::new();
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    state.settings.write().await.profiles[0].enabled = false;
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    assert_eq!(roots, vec![PathBuf::from(&b.input_dir)]);
    assert_eq!(watcher.unwatch_calls, vec![PathBuf::from(&a.input_dir)]);

    state.settings.write().await.monitoring_paused = true;
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    assert!(watcher.watched.is_empty());
    assert!(roots.is_empty());
    state.settings.write().await.monitoring_paused = false;
    reload_watches(&state, &mut watcher, &mut roots)
        .await
        .unwrap();
    assert_eq!(
        watcher.watched,
        HashSet::from([PathBuf::from(&b.input_dir)])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_with_missing_source_discovers_other_existing_files() {
    let (root, _state, a, b) = fixture().await;
    std::fs::rename(&a.input_dir, root.path().join("A-relocated")).unwrap();
    let existing = Path::new(&b.input_dir).join("existing-before-restart.md");
    std::fs::write(&existing, "# existing B").unwrap();
    let restarted = AppState::new(root.path().join("data")).unwrap();
    let monitor = RunningMonitor::start(&restarted);
    assert_eq!(wait_for_task(&restarted, &existing).await.profile_id, b.id);

    let fresh = Path::new(&b.input_dir).join("new-after-restart.md");
    std::fs::write(&fresh, "# new B").unwrap();
    assert_eq!(wait_for_task(&restarted, &fresh).await.profile_id, b.id);
    monitor.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn moving_and_reloading_one_source_keeps_other_files_discoverable() {
    let (root, state, a, b) = fixture().await;
    let seed = Path::new(&b.input_dir).join("seed.md");
    std::fs::write(&seed, "# seed B").unwrap();
    let monitor = RunningMonitor::start(&state);
    wait_for_task(&state, &seed).await;
    let relocated = root.path().join("A-relocated");
    std::fs::rename(&a.input_dir, &relocated).unwrap();
    state.send_runtime(RuntimeMessage::Reload).unwrap();
    let during_reload = Path::new(&b.input_dir).join("during-reload.md");
    std::fs::write(&during_reload, "# B during reload").unwrap();
    let task = wait_for_task(&state, &during_reload).await;
    assert_eq!(task.profile_id, b.id);
    assert_eq!(
        task.output_path,
        Some(
            Path::new(&b.output_dir)
                .join("during-reload.md")
                .to_string_lossy()
                .into_owned()
        )
    );

    state.settings.write().await.profiles[0].input_dir = dunce::canonicalize(&relocated)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    state.send_runtime(RuntimeMessage::Reload).unwrap();
    let a_after = dunce::canonicalize(&relocated).unwrap().join("a-after.md");
    std::fs::write(&a_after, "# relocated A").unwrap();
    assert_eq!(wait_for_task(&state, &a_after).await.profile_id, a.id);
    let b_after = Path::new(&b.input_dir).join("b-after.md");
    std::fs::write(&b_after, "# unchanged B").unwrap();
    assert_eq!(wait_for_task(&state, &b_after).await.profile_id, b.id);

    state.settings.write().await.paused = false;
    state.set_paused_flag(false);
    state.send_runtime(RuntimeMessage::ProcessQueued).unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if [&during_reload, &a_after, &b_after].iter().all(|source| {
                state
                    .storage
                    .find_by_source(&source.to_string_lossy())
                    .unwrap()
                    .is_some_and(|task| task.status == JobStatus::Completed)
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(Path::new(&b.output_dir).join("during-reload.md")).unwrap(),
        b"# B during reload"
    );
    assert_eq!(
        std::fs::read(Path::new(&b.output_dir).join("b-after.md")).unwrap(),
        b"# unchanged B"
    );
    assert_eq!(
        std::fs::read(Path::new(&a.output_dir).join("a-after.md")).unwrap(),
        b"# relocated A"
    );
    monitor.stop().await;
}
