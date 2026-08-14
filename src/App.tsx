import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertCircle, Check, X } from "lucide-react";

import {
  activeStatuses,
  emptySettings,
  errorMessage,
  makeProfile,
  pendingStatuses,
  profilesReadyToSave,
  settingsSaveSignature,
  previewDashboard,
  previewMode,
  useThemeMode,
  type DirectorySaveState,
  type Notice,
  type TaskFilter,
  type TagFilter,
  type View,
} from "@/app-model";
import { AppShell } from "@/components/app/app-shell";
import { DirectoriesView } from "@/components/app/directories-view";
import { FormatsView } from "@/components/app/formats-view";
import { HelpView } from "@/components/app/help-view";
import { OverviewView } from "@/components/app/overview-view";
import { SettingsView } from "@/components/app/settings-view";
import { TaskWorkspace } from "@/components/app/task-workspace";
import { TagTasksView } from "@/components/app/tag-tasks-view";
import { IconAction } from "@/components/app/icon-action";
import { TooltipProvider } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { AppSettings, Dashboard, HealthReport, TagJobRecord, TaggingConfig, TaggingImpact, TaskRecord, WatchProfile } from "./types";

export default function App() {
  const [settings, setSettings] = useState<AppSettings>(emptySettings);
  const [persistedSettings, setPersistedSettings] = useState<AppSettings>(emptySettings);
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [tagJobs, setTagJobs] = useState<TagJobRecord[]>([]);
  const [taskTotal, setTaskTotal] = useState(0);
  const [tagJobTotal, setTagJobTotal] = useState(0);
  const [activeView, setActiveView] = useState<View>("overview");
  const [taskFilter, setTaskFilter] = useState<TaskFilter>("all");
  const [tagFilter, setTagFilter] = useState<TagFilter>("all");
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [selectedTagId, setSelectedTagId] = useState<string | null>(null);
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(null);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [token, setToken] = useState("");
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [savingSettings, setSavingSettings] = useState(false);
  const [autoSaveError, setAutoSaveError] = useState("");
  const [savingToken, setSavingToken] = useState(false);
  const [changingMonitoringState, setChangingMonitoringState] = useState(false);
  const [rescanning, setRescanning] = useState(false);
  const [retryingFailed, setRetryingFailed] = useState(false);
  const [healthReport, setHealthReport] = useState<HealthReport | null>(null);
  const [appVersion, setAppVersion] = useState("1.1.0");
  const [checkingHealth, setCheckingHealth] = useState(false);
  const [copyingDiagnostics, setCopyingDiagnostics] = useState(false);
  const [pausing, setPausing] = useState(false);
  const [changingClassificationState, setChangingClassificationState] = useState(false);
  const [retryingIds, setRetryingIds] = useState<Set<string>>(new Set());
  const [retryingTagIds, setRetryingTagIds] = useState<Set<string>>(new Set());
  const [loadError, setLoadError] = useState("");
  const [notice, setNotice] = useState<Notice | null>(null);
  const commandInputRef = useRef<HTMLInputElement>(null);
  const failedAutoSaveSignatureRef = useRef<string | null>(null);
  const initialViewResolvedRef = useRef(false);
  const { theme, setTheme } = useThemeMode();

  const refresh = useCallback(async (includeSettings = false) => {
    try {
      const dashboard = previewMode ? previewDashboard : await invoke<Dashboard>("get_dashboard");
      if (includeSettings) {
        setSettings(dashboard.settings);
        setPersistedSettings(dashboard.settings);
        setAutoSaveError("");
        failedAutoSaveSignatureRef.current = null;
        if (!initialViewResolvedRef.current) {
          initialViewResolvedRef.current = true;
          if (dashboard.settings.profiles.length === 0) setActiveView("help");
        }
      }
      setTasks(dashboard.tasks);
      setTagJobs(dashboard.tagJobs);
      setTaskTotal(dashboard.taskTotal);
      setTagJobTotal(dashboard.tagJobTotal);
      setLoadError([dashboard.runtimeError, dashboard.tagRuntimeError, dashboard.indexRuntimeError].filter(Boolean).join("；"));
    } catch (error) {
      setLoadError(errorMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh(true);
  }, [refresh]);

  useEffect(() => {
    if (!previewMode) void getVersion().then(setAppVersion).catch(() => {});
  }, []);

  const pollDelay = tasks.some((task) => activeStatuses.includes(task.status))
    || tagJobs.some((job) => job.status === "reading" || job.status === "writing")
    ? 2000
    : 8000;

  useEffect(() => {
    if (previewMode) return;
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void refresh();
    }, pollDelay);
    const handleVisibility = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", handleVisibility);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [pollDelay, refresh]);

  useEffect(() => {
    if (!selectedTaskId && tasks.length > 0) {
      setSelectedTaskId(tasks.find((task) => activeStatuses.includes(task.status))?.id ?? tasks[0].id);
    } else if (selectedTaskId && !tasks.some((task) => task.id === selectedTaskId)) {
      setSelectedTaskId(tasks[0]?.id ?? null);
    }
  }, [selectedTaskId, tasks]);

  useEffect(() => {
    if (settings.profiles.some((profile) => profile.id === selectedProfileId)) return;
    setSelectedProfileId(
      settings.profiles.find((profile) => profile.tagging.enabled && profile.tagging.labels.length > 0)?.id
      ?? settings.profiles.find((profile) => profile.enabled)?.id
      ?? settings.profiles[0]?.id
      ?? null,
    );
  }, [selectedProfileId, settings.profiles]);

  useEffect(() => {
    if (!selectedTagId && tagJobs.length > 0) setSelectedTagId(tagJobs[0].id);
    else if (selectedTagId && !tagJobs.some((job) => job.id === selectedTagId)) setSelectedTagId(tagJobs[0]?.id ?? null);
  }, [selectedTagId, tagJobs]);

  useEffect(() => {
    function handleKeyboard(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLocaleLowerCase() === "k") {
        event.preventDefault();
        commandInputRef.current?.focus();
      }
      if (event.key === "Escape") {
        if (inspectorOpen) setInspectorOpen(false);
        else if (query) setQuery("");
      }
    }
    window.addEventListener("keydown", handleKeyboard);
    return () => window.removeEventListener("keydown", handleKeyboard);
  }, [inspectorOpen, query]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 3200);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const currentProfilesSignature = useMemo(() => settingsSaveSignature(settings), [settings]);
  const persistedProfilesSignature = useMemo(() => settingsSaveSignature(persistedSettings), [persistedSettings]);
  const directorySettingsDirty = currentProfilesSignature !== persistedProfilesSignature;
  const directorySettingsComplete = profilesReadyToSave(settings.profiles);
  const directorySaveState: DirectorySaveState = savingSettings
    ? "saving"
    : directorySettingsDirty && autoSaveError
      ? "error"
      : !directorySettingsDirty
        ? "saved"
        : directorySettingsComplete
          ? "dirty"
          : "incomplete";

  const counts = useMemo(() => ({
    enabled: persistedSettings.profiles.filter((profile) => profile.enabled).length,
    pending: tasks.filter((task) => pendingStatuses.includes(task.status)).length + tagJobs.filter((job) => job.status === "queued").length,
    active: tasks.filter((task) => activeStatuses.includes(task.status)).length + tagJobs.filter((job) => job.status === "reading" || job.status === "writing").length,
  }), [persistedSettings.profiles, tagJobs, tasks]);

  useEffect(() => {
    if (
      loading ||
      savingSettings ||
      !directorySettingsDirty ||
      !directorySettingsComplete ||
      failedAutoSaveSignatureRef.current === currentProfilesSignature
    ) {
      return;
    }
    const timer = window.setTimeout(() => void saveSettings(true, settings), 900);
    return () => window.clearTimeout(timer);
  }, [
    currentProfilesSignature,
    directorySettingsComplete,
    directorySettingsDirty,
    loading,
    savingSettings,
    settings,
  ]);

  async function manualRefresh() {
    setRefreshing(true);
    await refresh();
    setRefreshing(false);
  }

  function patchProfile(id: string, patch: Partial<WatchProfile>) {
    failedAutoSaveSignatureRef.current = null;
    setAutoSaveError("");
    setSettings((current) => ({
      ...current,
      profiles: current.profiles.map((profile) => (profile.id === id ? { ...profile, ...patch } : profile)),
    }));
  }

  function addProfile() {
    failedAutoSaveSignatureRef.current = null;
    setAutoSaveError("");
    const profile = makeProfile();
    setSettings((current) => ({ ...current, profiles: [...current.profiles, profile] }));
    return profile;
  }

  function removeProfile(id: string) {
    failedAutoSaveSignatureRef.current = null;
    setAutoSaveError("");
    setSettings((current) => ({ ...current, profiles: current.profiles.filter((profile) => profile.id !== id) }));
  }

  function toggleFormatExtensions(extensions: string[], enabled: boolean) {
    failedAutoSaveSignatureRef.current = null;
    setAutoSaveError("");
    setSettings((current) => {
      const next = new Set(current.enabledExtensions);
      for (const extension of extensions) {
        if (enabled) next.add(extension);
        else next.delete(extension);
      }
      return { ...current, enabledExtensions: [...next] };
    });
  }

  async function chooseDirectory(profileId: string, field: "inputDir" | "outputDir") {
    if (previewMode) {
      const selected = field === "inputDir" ? "C:\\Users\\Demo\\Documents\\待转换文档" : "D:\\MarkdownKnowledgeBase\\转换结果";
      patchProfile(profileId, { [field]: selected });
      return;
    }
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") patchProfile(profileId, { [field]: selected });
  }

  async function saveSettings(automatic = false, snapshot = settings) {
    const submittedSignature = settingsSaveSignature(snapshot);
    const containsNewProfile = snapshot.profiles.some(
      (profile) => !persistedSettings.profiles.some((persisted) => persisted.id === profile.id),
    );
    setSavingSettings(true);
    setAutoSaveError("");
    try {
      let saved: AppSettings;
      if (previewMode) {
        saved = snapshot;
      } else {
        saved = await invoke<AppSettings>("save_settings", { settings: snapshot });
      }
      setPersistedSettings(saved);
      setSettings((current) => (
        settingsSaveSignature(current) === submittedSignature ? saved : current
      ));
      failedAutoSaveSignatureRef.current = null;
      if (!automatic) {
        setNotice({ kind: "success", message: "设置已保存，监控任务已重新加载。" });
      } else if (containsNewProfile) {
        setNotice({ kind: "success", message: "新目录已自动保存，监听会扫描现有文件并加入待执行。" });
      }
    } catch (error) {
      const message = errorMessage(error);
      setAutoSaveError(message);
      if (automatic) failedAutoSaveSignatureRef.current = submittedSignature;
      setNotice({ kind: "error", message: automatic ? `自动保存失败：${message}` : message });
    } finally {
      setSavingSettings(false);
    }
  }

  async function saveToken() {
    if (!token.trim()) return;
    setSavingToken(true);
    try {
      if (previewMode) {
        setSettings((current) => ({ ...current, mineruConfigured: true }));
      } else {
        await invoke("set_mineru_token", { token: token.trim() });
        setSettings((current) => ({ ...current, mineruConfigured: true }));
        setPersistedSettings((current) => ({ ...current, mineruConfigured: true }));
      }
      setToken("");
      setNotice({ kind: "success", message: "MinerU Token 已安全保存。" });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    } finally {
      setSavingToken(false);
    }
  }

  async function openMineruTokenPage() {
    try {
      if (previewMode) {
        window.open("https://mineru.net/apiManage/token", "_blank", "noopener,noreferrer");
      } else {
        await invoke("open_mineru_token_page");
      }
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    }
  }

  async function saveAgent(value: { baseUrl: string; model: string; apiKey: string; concurrency: number }) {
    try {
      const agent = previewMode
        ? { baseUrl: value.baseUrl, model: value.model, concurrency: value.concurrency, configured: true }
        : await invoke<AppSettings["agent"]>("save_agent_settings", { baseUrl: value.baseUrl, model: value.model, apiKey: value.apiKey || null, concurrency: value.concurrency });
      setSettings((current) => ({ ...current, agent }));
      setPersistedSettings((current) => ({ ...current, agent }));
      setNotice({ kind: "success", message: "Agent 模型设置已安全保存。" });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
      throw error;
    }
  }

  async function testAgent(value: { baseUrl: string; model: string; apiKey: string }) {
    try {
      if (!previewMode) await invoke("test_agent_connection", { baseUrl: value.baseUrl, model: value.model, apiKey: value.apiKey || null });
      setNotice({ kind: "success", message: "连接成功，模型已完成 Tool Calling 测试。" });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
      throw error;
    }
  }

  async function previewTagging(profileId: string, tagging: TaggingConfig): Promise<TaggingImpact> {
    try {
      if (previewMode) return { discovered: 12, newFiles: 3, affected: 9 };
      return await invoke<TaggingImpact>("preview_tagging_change", { profileId, tagging });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
      throw error;
    }
  }

  async function applyTagging(profileId: string, tagging: TaggingConfig, processExisting: boolean) {
    try {
      const saved = previewMode
        ? { ...settings, profiles: settings.profiles.map((profile) => profile.id === profileId ? { ...profile, tagging } : profile) }
        : await invoke<AppSettings>("apply_tagging_config", { profileId, tagging, processExisting });
      setSettings(saved);
      setPersistedSettings(saved);
      setNotice({ kind: "success", message: tagging.enabled ? (processExisting ? "分类规则已应用，现有 Markdown 正在排队。" : "分类规则已应用，仅自动分类新文件。") : "该目录的 Agent 文档分类已关闭，待执行任务已取消。" });
      await refresh();
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
      throw error;
    }
  }

  async function retryTags(ids: string[]) {
    setRetryingTagIds((current) => new Set([...current, ...ids]));
    try {
      if (!previewMode) {
        if (ids.length === 1) await invoke("retry_tag_job", { jobId: ids[0] });
        else await invoke("retry_tag_jobs", { jobIds: ids });
        await refresh();
      }
      setNotice({
        kind: "success",
        message: settings.classificationPaused
          ? `已将 ${ids.length} 个任务加入待执行；开始分类后运行。`
          : `已重新提交 ${ids.length} 个分类任务。`,
      });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    } finally {
      setRetryingTagIds((current) => {
        const next = new Set(current);
        ids.forEach((id) => next.delete(id));
        return next;
      });
    }
  }

  async function retryTask(task: TaskRecord) {
    setRetryingIds((current) => new Set(current).add(task.id));
    try {
      if (previewMode) {
        setTasks((current) => current.map((item) => item.id === task.id ? { ...item, status: "queued", error: undefined } : item));
      } else {
        await invoke("retry_task", { taskId: task.id, forceLocal: false });
        await refresh();
      }
      setNotice({ kind: "success", message: `已重新提交「${task.relativePath}」。` });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    } finally {
      setRetryingIds((current) => {
        const next = new Set(current);
        next.delete(task.id);
        return next;
      });
    }
  }

  async function retryFailedTasks() {
    setRetryingFailed(true);
    try {
      const count = previewMode
        ? tasks.filter((task) => task.status === "failed").length
        : await invoke<number>("retry_failed_tasks");
      await refresh();
      setNotice({
        kind: "success",
        message: settings.paused
          ? `已将 ${count} 个失败任务放回待执行；开始转换后运行。`
          : `已重新提交 ${count} 个失败任务。`,
      });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    } finally {
      setRetryingFailed(false);
    }
  }

  async function rescanProfiles() {
    setRescanning(true);
    try {
      if (!previewMode) await invoke("rescan_all_profiles");
      await refresh();
      setNotice({ kind: "success", message: "已重新扫描监控目录，新文件会进入待执行。" });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    } finally {
      setRescanning(false);
    }
  }

  async function runHealthCheck() {
    setCheckingHealth(true);
    try {
      const report = previewMode
        ? { appVersion: "1.1.0", checkedAt: new Date().toISOString(), overall: "ok", checks: [{ id: "preview", title: "预览模式", level: "ok", detail: "界面预览正常。" }], counts: { conversionPending: 0, conversionActive: 0, conversionWaitingMineru: 0, conversionFailed: 0, classificationPending: 0, classificationActive: 0, classificationFailed: 0, classificationOutdated: 0 } } as HealthReport
        : await invoke<HealthReport>("run_health_check");
      setHealthReport(report);
      setNotice({ kind: "success", message: report.overall === "ok" ? "运行检查完成，所有项目正常。" : "运行检查完成，请查看诊断结果。" });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    } finally {
      setCheckingHealth(false);
    }
  }

  async function copyDiagnostics() {
    setCopyingDiagnostics(true);
    try {
      const report = previewMode ? "CPAH Docs 诊断信息\n版本: 1.1.0\n预览模式" : await invoke<string>("get_diagnostic_report");
      await navigator.clipboard.writeText(report);
      setNotice({ kind: "success", message: "诊断信息已复制，凭据和完整路径已隐藏。" });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    } finally {
      setCopyingDiagnostics(false);
    }
  }

  async function togglePaused() {
    setPausing(true);
    try {
      if (previewMode) {
        setSettings((current) => ({ ...current, paused: !current.paused }));
      } else {
        const saved = await invoke<AppSettings>("set_paused", { paused: !settings.paused });
        setSettings((current) => ({ ...current, paused: saved.paused }));
        setPersistedSettings((current) => ({ ...current, paused: saved.paused }));
      }
      const nextPaused = !settings.paused;
      setNotice({
        kind: "success",
        message: nextPaused
          ? "转换已停止；目录监听仍会把新文件加入待执行，已提交的 MinerU 任务会正常完成。"
          : "转换已开始；正在处理待执行队列。",
      });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    } finally {
      setPausing(false);
    }
  }

  async function toggleMonitoringPaused() {
    const paused = !settings.monitoringPaused;
    setChangingMonitoringState(true);
    try {
      const saved = previewMode
        ? { ...settings, monitoringPaused: paused }
        : await invoke<AppSettings>("set_monitoring_paused", { paused });
      setSettings((current) => ({ ...current, monitoringPaused: saved.monitoringPaused }));
      setPersistedSettings((current) => ({ ...current, monitoringPaused: saved.monitoringPaused }));
      setNotice({
        kind: "success",
        message: saved.monitoringPaused
          ? "目录监听已停止；已有待执行任务不受影响。"
          : "目录监听已开始；正在扫描并把发现的文件加入待执行。",
      });
      if (!saved.monitoringPaused) await refresh();
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    } finally {
      setChangingMonitoringState(false);
    }
  }

  async function toggleClassificationPaused() {
    const paused = !settings.classificationPaused;
    setChangingClassificationState(true);
    try {
      const saved = previewMode
        ? { ...settings, classificationPaused: paused }
        : await invoke<AppSettings>("set_classification_paused", { paused });
      setSettings((current) => ({ ...current, classificationPaused: saved.classificationPaused }));
      setPersistedSettings((current) => ({ ...current, classificationPaused: saved.classificationPaused }));
      if (!saved.classificationPaused) await refresh();
      setNotice({
        kind: "success",
        message: saved.classificationPaused
          ? "分类已停止；正在执行的 Agent 会正常完成。"
          : "分类已开始；正在扫描遗漏文件并处理待分类任务。",
      });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    } finally {
      setChangingClassificationState(false);
    }
  }

  async function openLocalPath(path: string) {
    try {
      if (!previewMode) await invoke("open_managed_path", { path });
    } catch (error) {
      setNotice({ kind: "error", message: errorMessage(error) });
    }
  }

  function changeQuery(value: string) {
    setQuery(value);
    if (value && activeView !== "tagging") setActiveView("tasks");
  }

  function selectTask(taskId: string) {
    setSelectedTaskId(taskId);
    setInspectorOpen(true);
  }

  function selectTag(tagId: string) {
    setSelectedTagId(tagId);
    setInspectorOpen(true);
  }

  function showTasks(taskId?: string) {
    if (taskId) setSelectedTaskId(taskId);
    setActiveView("tasks");
  }

  function openDirectorySetup() {
    if (settings.profiles.length === 0) {
      const profile = addProfile();
      setSelectedProfileId(profile.id);
    }
    setActiveView("directories");
  }

  const viewContent = activeView === "overview" ? (
    <OverviewView
      tasks={tasks}
      tagJobs={tagJobs}
      taskTotal={taskTotal}
      profiles={settings.profiles}
      persistedProfiles={persistedSettings.profiles}
      saveState={directorySaveState}
      monitoringPaused={settings.monitoringPaused}
      conversionPaused={settings.paused}
      classificationPaused={settings.classificationPaused}
      agentConfigured={settings.agent.configured}
      onOpenTasks={showTasks}
      onOpenDirectories={() => setActiveView("directories")}
      onOpenClassification={() => setActiveView("tagging")}
    />
  ) : activeView === "directories" ? (
    <DirectoriesView
      profiles={settings.profiles}
      persistedProfiles={persistedSettings.profiles}
      selectedId={selectedProfileId}
      onSelectedIdChange={setSelectedProfileId}
      saving={savingSettings}
      saveState={directorySaveState}
      saveError={autoSaveError}
      monitoringPaused={settings.monitoringPaused}
      changingMonitoringState={changingMonitoringState}
      onToggleMonitoringPaused={() => void toggleMonitoringPaused()}
      onAdd={addProfile}
      onPatch={patchProfile}
      onRemove={removeProfile}
      onChooseDirectory={(id, field) => void chooseDirectory(id, field)}
      onOpenDirectory={(path) => void openLocalPath(path)}
      onSave={() => void saveSettings(false)}
      agentConfigured={settings.agent.configured}
      onPreviewTagging={previewTagging}
      onApplyTagging={applyTagging}
      onOpenTagTasks={() => setActiveView("tagging")}
      onOpenSettings={() => setActiveView("settings")}
    />
  ) : activeView === "settings" ? (
    <SettingsView
      appVersion={appVersion}
      theme={theme}
      onThemeChange={setTheme}
      mineruConfigured={settings.mineruConfigured}
      mineruBaseUrl={settings.mineruBaseUrl}
      token={token}
      onTokenChange={setToken}
      savingToken={savingToken}
      onSaveToken={() => void saveToken()}
      onOpenMineruTokenPage={() => void openMineruTokenPage()}
      agent={settings.agent}
      onSaveAgent={saveAgent}
      onTestAgent={testAgent}
    />
  ) : activeView === "tagging" ? (
    <TagTasksView
      jobs={tagJobs}
      total={tagJobTotal}
      profiles={settings.profiles}
      filter={tagFilter}
      onFilterChange={setTagFilter}
      query={query}
      selectedId={selectedTagId}
      onSelect={selectTag}
      retryingIds={retryingTagIds}
      onRetry={(ids) => void retryTags(ids)}
      onOpen={(path) => void openLocalPath(path)}
      onRefresh={() => void manualRefresh()}
      refreshing={refreshing}
      loading={loading}
      classificationPaused={settings.classificationPaused}
      agentConfigured={settings.agent.configured}
      changingClassificationState={changingClassificationState}
      onToggleClassificationPaused={() => void toggleClassificationPaused()}
      onOpenSettings={() => setActiveView("settings")}
      inspectorOpen={inspectorOpen}
      onCloseInspector={() => setInspectorOpen(false)}
    />
  ) : activeView === "formats" ? (
    <FormatsView
      mineruConfigured={settings.mineruConfigured}
      enabledExtensions={settings.enabledExtensions}
      saveState={directorySaveState}
      onToggleExtensions={toggleFormatExtensions}
      onOpenSettings={() => setActiveView("settings")}
    />
  ) : activeView === "help" ? (
    <HelpView
      hasProfiles={settings.profiles.length > 0}
      onStartSetup={openDirectorySetup}
      onOpenFormats={() => setActiveView("formats")}
      onOpenSettings={() => setActiveView("settings")}
      onOpenConversionTasks={() => setActiveView("tasks")}
      onOpenClassificationTasks={() => setActiveView("tagging")}
      healthReport={healthReport}
      appVersion={appVersion}
      checkingHealth={checkingHealth}
      copyingDiagnostics={copyingDiagnostics}
      onRunHealthCheck={() => void runHealthCheck()}
      onCopyDiagnostics={() => void copyDiagnostics()}
      onLoadThirdPartyLicenses={() => previewMode ? Promise.resolve("预览模式不载入第三方许可证正文。") : invoke<string>("get_third_party_licenses")}
    />
  ) : (
    <TaskWorkspace
      tasks={tasks}
      total={taskTotal}
      profiles={settings.profiles}
      filter={taskFilter}
      onFilterChange={setTaskFilter}
      query={query}
      selectedTaskId={selectedTaskId}
      onSelectTask={selectTask}
      inspectorOpen={inspectorOpen}
      onCloseInspector={() => setInspectorOpen(false)}
      retryingIds={retryingIds}
      onRetry={(task) => void retryTask(task)}
      onOpenResult={(path) => void openLocalPath(path)}
      onRefresh={() => void manualRefresh()}
      onRescan={() => void rescanProfiles()}
      rescanning={rescanning}
      onRetryFailed={() => void retryFailedTasks()}
      retryingFailed={retryingFailed}
      refreshing={refreshing}
      loading={loading}
      paused={settings.paused}
      pausing={pausing}
      onTogglePaused={() => void togglePaused()}
    />
  );

  return (
    <TooltipProvider>
      <AppShell
        activeView={activeView}
        onViewChange={(view) => { setActiveView(view); setInspectorOpen(false); }}
        taskCount={taskTotal}
        tagJobCount={tagJobTotal}
        pendingCount={counts.pending}
        activeCount={counts.active}
        enabledDirectories={counts.enabled}
        monitoringPaused={settings.monitoringPaused}
        loading={loading}
        loadError={loadError}
        refreshing={refreshing}
        onRefresh={() => void manualRefresh()}
        query={query}
        onQueryChange={changeQuery}
        commandInputRef={commandInputRef}
        previewNativeBar={previewMode}
      >
        {viewContent}
      </AppShell>

      {loadError && (
        <div className="fixed bottom-4 left-1/2 z-50 flex max-w-[520px] -translate-x-1/2 items-center gap-2 rounded-md border border-destructive/30 bg-card px-3 py-2 text-[11px] text-destructive shadow-lg">
          <AlertCircle className="size-3.5 shrink-0" /><span className="truncate">{loadError}</span>
        </div>
      )}

      {notice && (
        <div className={cn("fixed right-4 top-16 z-50 flex max-w-sm items-center gap-2 rounded-md border bg-card px-3 py-2 text-[11px] shadow-lg", notice.kind === "error" ? "border-destructive/30 text-destructive" : "border-success/30 text-foreground")}>
          {notice.kind === "error" ? <AlertCircle className="size-3.5 shrink-0" /> : <Check className="size-3.5 shrink-0 text-success" />}
          <span className="leading-5">{notice.message}</span>
          <IconAction label="关闭通知" size="icon-sm" onClick={() => setNotice(null)}><X /></IconAction>
        </div>
      )}
    </TooltipProvider>
  );
}
