import { useEffect, useState } from "react";

import type { AppSettings, Dashboard, JobStatus, TagJobStatus, TaskRecord, WatchProfile } from "./types";

export type View = "overview" | "directories" | "tasks" | "tagging" | "formats" | "help" | "settings";
export type TaskFilter = "all" | "pending" | "active" | "completed" | "failed";
export type TagFilter = "all" | "queued" | "active" | "completed" | "failed" | "outdated";
export type ThemeMode = "system" | "light" | "dark";
export type Notice = { kind: "success" | "error"; message: string };
export type DirectorySaveState = "saved" | "dirty" | "incomplete" | "saving" | "error";

export const emptySettings: AppSettings = {
  profiles: [],
  monitoringPaused: false,
  paused: true,
  classificationPaused: true,
  mineruBaseUrl: "https://mineru.net/api/v4",
  mineruConfigured: false,
  enabledExtensions: ["md", "docx", "xlsx", "xls", "pptx", "html", "htm", "csv", "txt", "pdf", "doc", "ppt", "png", "jpg", "jpeg", "webp", "bmp"],
  splitEnabled: true,
  splitMaxPages: 200,
  splitOverlapPages: 5,
  splitTempDir: null,
  splitKeepTemp: false,
  agent: { baseUrl: "https://api.openai.com/v1", model: "", configured: false, concurrency: 1 },
};

export const activeStatuses: JobStatus[] = [
  "converting",
  "uploading",
  "processing",
  "downloading",
];

export const pendingStatuses: JobStatus[] = ["waiting_stable", "queued", "waiting_mineru"];

export const statusMeta: Record<JobStatus, { label: string; tone: "neutral" | "active" | "success" | "danger" }> = {
  waiting_stable: { label: "等待稳定", tone: "neutral" },
  queued: { label: "待执行", tone: "neutral" },
  converting: { label: "本地转换", tone: "active" },
  waiting_mineru: { label: "等待 MinerU", tone: "neutral" },
  uploading: { label: "正在上传", tone: "active" },
  processing: { label: "云端解析", tone: "active" },
  downloading: { label: "下载结果", tone: "active" },
  completed: { label: "已完成", tone: "success" },
  failed: { label: "失败", tone: "danger" },
};

export const tagStatusMeta: Record<TagJobStatus, { label: string; tone: "neutral" | "active" | "success" | "danger" }> = {
  queued: { label: "待执行", tone: "neutral" },
  reading: { label: "Agent 读取", tone: "active" },
  writing: { label: "写入 YAML", tone: "active" },
  completed: { label: "分类完成", tone: "success" },
  failed: { label: "分类失败", tone: "danger" },
  outdated: { label: "分类已过期", tone: "neutral" },
  cancelled: { label: "已取消", tone: "neutral" },
};

export const previewMode = import.meta.env.DEV && new URLSearchParams(window.location.search).has("preview");

export const previewDashboard: Dashboard = {
  settings: {
    monitoringPaused: false,
    paused: false,
    classificationPaused: false,
    mineruBaseUrl: "https://mineru.net/api/v4",
    mineruConfigured: true,
    enabledExtensions: ["md", "docx", "xlsx", "xls", "pptx", "html", "htm", "csv", "txt", "pdf", "doc", "ppt", "png", "jpg", "jpeg", "webp", "bmp"],
    splitEnabled: true,
    splitMaxPages: 200,
    splitOverlapPages: 5,
    splitTempDir: null,
    splitKeepTemp: false,
    agent: { baseUrl: "https://api.openai.com/v1", model: "gpt-4.1-mini", configured: true, concurrency: 1 },
    profiles: [
      {
        id: "profile-finance",
        name: "财务与公告",
        inputDir: "C:\\Users\\Demo\\Documents\\公司资料\\财务与公告",
        outputDir: "D:\\KnowledgeBase\\财务与公告",
        enabled: true,
        deletePolicy: "trash",
        tagging: { enabled: true, selectionMode: "multiple", labels: [{ id: "training", name: "培训材料", description: "课程、讲义和培训案例" }, { id: "audit", name: "审计资料", description: "审计方案、底稿与审计方法" }, { id: "regulation", name: "法规制度", description: "法律法规、监管规则和内部制度" }] },
      },
      {
        id: "profile-projects",
        name: "项目文档",
        inputDir: "D:\\Projects\\共享项目资料\\2026年度重点项目",
        outputDir: "D:\\KnowledgeBase\\项目文档",
        enabled: true,
        deletePolicy: "keep",
        tagging: { enabled: false, selectionMode: "single", labels: [] },
      },
      {
        id: "profile-archive",
        name: "历史归档",
        inputDir: "E:\\Archive\\OfficeDocuments",
        outputDir: "D:\\KnowledgeBase\\历史归档",
        enabled: false,
        deletePolicy: "keep",
        tagging: { enabled: false, selectionMode: "single", labels: [] },
      },
    ],
  },
  tasks: [
    {
      id: "task-1",
      profileId: "profile-finance",
      sourcePath: "C:\\Users\\Demo\\Documents\\公司资料\\财务与公告\\2026年半年度报告.pdf",
      relativePath: "2026年半年度报告.pdf",
      engine: "mineru",
      status: "completed",
      outputPath: "D:\\KnowledgeBase\\财务与公告\\2026年半年度报告.md",
      updatedAt: "2026-08-11T09:42:00+08:00",
    },
    {
      id: "task-2",
      profileId: "profile-projects",
      sourcePath: "D:\\Projects\\共享项目资料\\2026年度重点项目\\实施方案终稿.docx",
      relativePath: "实施方案终稿.docx",
      engine: "anytomd",
      status: "converting",
      updatedAt: "2026-08-11T09:40:00+08:00",
    },
    {
      id: "task-3",
      profileId: "profile-finance",
      sourcePath: "C:\\Users\\Demo\\Documents\\公司资料\\财务与公告\\经营分析数据.xlsx",
      relativePath: "经营分析数据.xlsx",
      engine: "anytomd",
      status: "failed",
      error: "工作簿已加密，无法读取内容。请移除密码后重试。",
      updatedAt: "2026-08-11T09:36:00+08:00",
    },
    {
      id: "task-4",
      profileId: "profile-projects",
      sourcePath: "D:\\Projects\\共享项目资料\\2026年度重点项目\\产品路线图.pptx",
      relativePath: "产品路线图.pptx",
      engine: "anytomd",
      status: "queued",
      updatedAt: "2026-08-11T09:33:00+08:00",
    },
    {
      id: "task-5",
      profileId: "profile-finance",
      sourcePath: "C:\\Users\\Demo\\Documents\\公司资料\\财务与公告\\合同与补充协议\\扫描合同.pdf",
      relativePath: "合同与补充协议/扫描合同.pdf",
      engine: "mineru",
      status: "processing",
      mineruState: "running",
      mineruExtractedPages: 37,
      mineruTotalPages: 64,
      updatedAt: "2026-08-11T09:31:00+08:00",
    },
    {
      id: "task-6",
      profileId: "profile-finance",
      sourcePath: "C:\\Users\\Demo\\Documents\\公司资料\\财务与公告\\季度经营简报.docx",
      relativePath: "季度经营简报.docx",
      engine: "anytomd",
      status: "completed",
      outputPath: "D:\\KnowledgeBase\\财务与公告\\季度经营简报.md",
      updatedAt: "2026-08-11T09:12:00+08:00",
    },
  ],
  tagJobs: [
    { id: "tag-1", profileId: "profile-finance", markdownPath: "D:\\KnowledgeBase\\财务与公告\\2026年半年度报告.md", relativePath: "2026年半年度报告.md", status: "completed", contentHash: "demo", schemaHash: "demo", resultJson: "[\"审计资料\"]", readBytes: 8192, totalBytes: 18600, apiCalls: 1, inputTokens: 1620, outputTokens: 18, updatedAt: "2026-08-12T09:43:00+08:00" },
    { id: "tag-2", profileId: "profile-finance", markdownPath: "D:\\KnowledgeBase\\财务与公告\\经营分析数据.md", relativePath: "经营分析数据.md", status: "failed", schemaHash: "demo", error: "模型未调用分类工具", readBytes: 4096, totalBytes: 12200, apiCalls: 1, inputTokens: 980, outputTokens: 42, updatedAt: "2026-08-12T09:37:00+08:00" },
    { id: "tag-3", profileId: "profile-projects", markdownPath: "D:\\KnowledgeBase\\项目文档\\产品路线图.md", relativePath: "产品路线图.md", status: "outdated", schemaHash: "old-demo", error: "分类规则已变化", readBytes: 0, totalBytes: 0, apiCalls: 0, inputTokens: 0, outputTokens: 0, updatedAt: "2026-08-12T09:34:00+08:00" },
  ],
  taskTotal: 6,
  tagJobTotal: 3,
  runtimeError: null,
  tagRuntimeError: null,
  indexRuntimeError: null,
};

export function makeProfile(): WatchProfile {
  return {
    id: crypto.randomUUID(),
    name: "新监控目录",
    inputDir: "",
    outputDir: "",
    enabled: true,
    deletePolicy: "trash",
    tagging: { enabled: false, selectionMode: "single", labels: [] },
  };
}

export function profileSignature(profile: WatchProfile) {
  return JSON.stringify([
    profile.id,
    profile.name,
    profile.inputDir,
    profile.outputDir,
    profile.enabled,
    profile.deletePolicy,
    profile.tagging,
  ]);
}

export function profilesSignature(profiles: WatchProfile[]) {
  return JSON.stringify(profiles.map(profileSignature));
}

export function settingsSaveSignature(settings: AppSettings) {
  return JSON.stringify([
    profilesSignature(settings.profiles),
    settings.enabledExtensions,
  ]);
}

export function profilesReadyToSave(profiles: WatchProfile[]) {
  return profiles.every((profile) => {
    if (!profile.inputDir.trim() || !profile.outputDir.trim()) return false;
    const names = profile.tagging.labels.map((label) => label.name.trim());
    if (names.some((name) => !name || name === "未分类")) return false;
    if (new Set(names.map((name) => name.toLocaleLowerCase())).size !== names.length) return false;
    return !profile.tagging.enabled || names.length > 0;
  });
}

export function profileIsPersisted(profile: WatchProfile, persistedProfiles: WatchProfile[]) {
  const persisted = persistedProfiles.find((candidate) => candidate.id === profile.id);
  return persisted !== undefined && profileSignature(persisted) === profileSignature(profile);
}

export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function formatUpdatedAt(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(date);
}

export function taskFileName(task: TaskRecord) {
  return task.relativePath.split(/[\\/]/).at(-1) ?? task.relativePath;
}

export function taskDirectory(task: TaskRecord) {
  const pieces = task.relativePath.split(/[\\/]/);
  return pieces.length > 1 ? pieces.slice(0, -1).join(" / ") : "根目录";
}

export function isMarkdownTask(task: TaskRecord) {
  return task.relativePath.toLocaleLowerCase().endsWith(".md");
}

export function useThemeMode() {
  const [theme, setTheme] = useState<ThemeMode>(() => {
    const saved = window.localStorage.getItem("cpahelper-theme");
    return saved === "light" || saved === "dark" || saved === "system" ? saved : "system";
  });

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const applyTheme = () => {
      const dark = theme === "dark" || (theme === "system" && media.matches);
      document.documentElement.classList.toggle("dark", dark);
    };
    window.localStorage.setItem("cpahelper-theme", theme);
    applyTheme();
    media.addEventListener("change", applyTheme);
    return () => media.removeEventListener("change", applyTheme);
  }, [theme]);

  return { theme, setTheme };
}
