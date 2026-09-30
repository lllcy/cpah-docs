import { t, getLocale } from "./i18n.ts";
import type { FileBrowserResult, FileEntry, FileFilter, TaskRecord, WatchProfile } from "./types";

export const fileFilters: { value: FileFilter; label: string }[] = [
  { value: "all", label: "全部状态" }, { value: "pending", label: "待执行" },
  { value: "active", label: "转换中" }, { value: "completed", label: "已完成" },
  { value: "failed", label: "失败" }, { value: "untracked", label: "未加入任务" },
  { value: "disabled", label: "格式未启用" }, { value: "unsupported", label: "不支持" },
  { value: "unreadable", label: "无法读取" },
];

export function parentPath(path: string) {
  const index = path.lastIndexOf("/");
  return index < 0 ? "" : path.slice(0, index);
}

export function absoluteFilePath(root: string, relativePath: string) {
  const separator = root.includes("\\") ? "\\" : "/";
  return root.replace(/[\\/]+$/, "") + (relativePath ? separator + relativePath.replaceAll("/", separator) : "");
}

export function readableSize(size: number | null) {
  if (size === null) return "—";
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KiB`;
  return `${(size / 1024 / 1024).toFixed(1)} MiB`;
}

export function entryFilter(entry: FileEntry): FileFilter {
  if (entry.availability === "unreadable" && entry.kind !== "directory") return "unreadable";
  if (entry.task) {
    switch (entry.task.status) {
      case "completed": return "completed";
      case "failed": return "failed";
      case "queued": case "waiting_stable": case "waiting_mineru": return "pending";
      default: return "active";
    }
  }
  switch (entry.availability) {
    case "eligible": return "untracked";
    case "disabled": return "disabled";
    case "unsupported": return "unsupported";
    default: return "all";
  }
}

const collators = { en: new Intl.Collator("en", { numeric: true, sensitivity: "base" }), "zh-CN": new Intl.Collator("zh-CN", { numeric: true, sensitivity: "base" }) };
export function compareEntries(a: FileEntry, b: FileEntry) {
  return Number(b.kind === "directory") - Number(a.kind === "directory")
    || collators[getLocale()].compare(a.name, b.name) || a.relativePath.localeCompare(b.relativePath);
}

export function expandedDirectories(expanded: Set<string>) {
  return ["", ...[...expanded].filter((path) => {
    let parent = parentPath(path);
    while (parent) {
      if (!expanded.has(parent)) return false;
      parent = parentPath(parent);
    }
    return true;
  })];
}

export type BrowserRow = { key: string; path: string; depth: number; entry?: FileEntry; message?: string; error?: boolean };
export function flattenEntries(entries: FileEntry[], expanded: Set<string>, directories: FileBrowserResult["directories"], searching: boolean): BrowserRow[] {
  const children = new Map<string, FileEntry[]>();
  for (const entry of entries) {
    const parent = parentPath(entry.relativePath);
    const group = children.get(parent) ?? [];
    group.push(entry);
    children.set(parent, group);
  }
  children.forEach((group) => group.sort(compareEntries));
  const loaded = new Map(directories.map((item) => [item.relativePath, item.error]));
  const result: BrowserRow[] = [];
  const stack = [...(children.get("") ?? [])].reverse().map((entry) => ({ entry, depth: 1 }));
  while (stack.length) {
    const { entry, depth } = stack.pop()!;
    result.push({ key: entry.relativePath, path: entry.relativePath, entry, depth });
    if (entry.kind !== "directory" || !expanded.has(entry.relativePath)) continue;
    const nested = children.get(entry.relativePath) ?? [];
    if (loaded.get(entry.relativePath)) {
      result.push({ key: `${entry.relativePath}/:error`, path: entry.relativePath, depth: depth + 1, message: loaded.get(entry.relativePath)!, error: true });
    } else if (!nested.length) {
      result.push({ key: `${entry.relativePath}/:empty`, path: entry.relativePath, depth: depth + 1, message: searching || loaded.has(entry.relativePath) ? t("空文件夹") : t("正在读取…") });
    }
    for (let index = nested.length - 1; index >= 0; index--) stack.push({ entry: nested[index], depth: depth + 1 });
  }
  return result;
}

// Only used by the existing development preview; no filesystem or task mutation.
export function previewFileEntries(profile: WatchProfile, tasks: TaskRecord[], enabledExtensions: string[]): FileEntry[] {
  const entries = new Map<string, FileEntry>();
  const add = (path: string, kind: FileEntry["kind"], task: TaskRecord | null = null) => {
    const relativePath = path.replaceAll("\\", "/");
    if (!task && entries.has(relativePath)) return;
    entries.set(relativePath, {
      relativePath, name: relativePath.split("/").at(-1)!, kind,
      size: kind === "file" ? task?.sourceSize ?? 124000 : null,
      modifiedMs: 1790568000000,
      availability: kind === "directory" ? "unreadable" : "eligible", task,
      sourceChanged: false, outputAvailable: task?.status === "completed", error: null,
    });
    let parent = parentPath(relativePath);
    while (parent) {
      if (!entries.has(parent)) add(parent, "directory");
      parent = parentPath(parent);
    }
  };
  tasks.filter((task) => task.profileId === profile.id && task.kind === "document").forEach((task) => add(task.relativePath, "file", task));
  add("待补充资料", "directory");
  add("历史公告/2025 年/资料说明.txt", "file");
  add("新收到的说明.docx", "file");
  add("原始附件.zip", "file");
  entries.get("原始附件.zip")!.availability = "unsupported";
  add("现场照片.jpg", "file");
  entries.get("现场照片.jpg")!.availability = enabledExtensions.includes("jpg") ? "eligible" : "disabled";
  // Reproducible large-tree UI check without personal data or a production setting.
  const count = Math.min(20000, Number(new URLSearchParams(window.location.search).get("previewFiles")) || 0);
  for (let index = 0; index < count; index++) add(`批量样例/样例文档-${index + 1}.txt`, "file");
  return [...entries.values()];
}

export function previewSearch(entries: FileEntry[], query: string, filter: FileFilter): FileBrowserResult {
  const matched = entries.filter((entry) => entry.kind !== "directory" && entry.name.toLowerCase().includes(query.trim().toLowerCase()) && (filter === "all" || entryFilter(entry) === filter));
  const keep = new Set(matched.map((entry) => entry.relativePath));
  for (const entry of matched) {
    let parent = parentPath(entry.relativePath);
    while (parent) { keep.add(parent); parent = parentPath(parent); }
  }
  return { entries: entries.filter((entry) => keep.has(entry.relativePath)), directories: [], missingPaths: [], matchedCount: matched.length };
}
