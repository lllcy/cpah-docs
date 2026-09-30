import { t, translate, getLocale } from "@/i18n";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertCircle, ArrowUpRight, ChevronDown, ChevronRight, FileText, Folder, FolderOpen, FoldVertical, Link2, LoaderCircle, RefreshCw, Play, Tags, Search, X } from "lucide-react";

import { activeStatuses, conversionEngineLabel, errorMessage, formatUpdatedAt, previewMode, tagStatusMeta } from "@/app-model";
import { absoluteFilePath, expandedDirectories, fileFilters, flattenEntries, parentPath, previewFileEntries, previewSearch, readableSize } from "@/file-browser-model";
import { IconAction } from "@/components/app/icon-action";
import { TaskStatus } from "@/components/app/task-status";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import type { FileAction, FileBrowserResult, FileEntry, FileFilter, TaskRecord, WatchProfile } from "@/types";

type Props = {
  profile: WatchProfile;
  enabledExtensions: string[];
  tasks: TaskRecord[];
  conversionPaused: boolean;
  monitoringPaused: boolean;
  classificationPaused: boolean;
  agentConfigured: boolean;
  onOpen: (path: string) => void;
  onFileAction: (profileId: string, entry: FileEntry, action: FileAction) => Promise<void>;
};

type Session = { expanded: string[]; query: string; filter: FileFilter; scrollTop: number };
const sessions = new Map<string, Session>();
const EMPTY: FileBrowserResult = { entries: [], directories: [], missingPaths: [], matchedCount: 0 };
const ROW_HEIGHT = 36;
// Coalesce obsolete searches while a previous disk scan is still in progress.
let searchQueue: Promise<unknown> = Promise.resolve();

function EntryStatus({ entry }: { entry: FileEntry }) {
  if (entry.kind === "directory") return null;
  if (entry.error) return <span className="text-[11px] text-destructive">{t("无法读取")}</span>;
  if (entry.task) return <TaskStatus status={entry.task.status} />;
  const label = { eligible: t("未加入任务"), disabled: t("格式未启用"), unsupported: t("不支持"), link: t("链接 · 不转换"), unreadable: t("无法读取") }[entry.availability];
  return <span className="text-[11px] text-muted-foreground">{label}</span>;
}

function FileDetails({ entry, profile, conversionPaused, classificationPaused, agentConfigured, onOpen, onFileAction, onClose }: {
  entry: FileEntry; profile: WatchProfile; conversionPaused: boolean; classificationPaused: boolean; agentConfigured: boolean;
  onOpen: Props["onOpen"]; onFileAction: Props["onFileAction"]; onClose: () => void;
}) {
  const drawer = useRef<HTMLElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => { close.current?.focus(); }, []);
  const path = absoluteFilePath(profile.inputDir, entry.relativePath);
  const canOpen = entry.kind === "file" && !entry.error;
  const task = entry.task;
  const [busy, setBusy] = useState<FileAction | null>(null);
  const [actionError, setActionError] = useState("");
  const converting = !!task && activeStatuses.includes(task.status) && task.status !== "waiting_parts";
  const classifying = task?.tagStatus === "reading" || task?.tagStatus === "writing";
  const blocked = !profile.enabled ? t("请先启用该目录。") : !canOpen ? t("请选择可读取的普通文件。") : entry.availability === "disabled" ? t("请先在“格式说明”中启用此格式。") : entry.availability !== "eligible" ? t("此格式不支持转换。") : "";
  const convertBlocked = blocked || (converting ? t("此文件正在转换。") : classifying ? t("请等待当前分类完成。") : "");
  const classifyBlocked = blocked || (classifying ? t("此文件正在分类。") : task?.status !== "completed" || !entry.outputAvailable ? t("请先转换此文件，再进行分类。") : entry.sourceChanged ? t("源文件已变化，请先重新转换。") : !profile.tagging.enabled || !profile.tagging.labels.length ? t("请在目录设置中开启分类并配置类别。") : !agentConfigured ? t("请先在设置中配置分类模型。") : "");
  const convertLabel = converting ? t("转换中") : task?.status === "completed" ? t("重新转换") : task?.status === "failed" ? t("重试转换") : task?.status === "waiting_parts" ? t("继续转换") : task ? t("优先转换") : t("转换");
  const classifyLabel = classifying ? t("分类中") : task?.tagStatus === "completed" ? t("重新分类") : task?.tagStatus === "failed" ? t("重试分类") : task?.tagStatus === "queued" ? t("优先分类") : t("分类");
  async function runAction(action: FileAction) {
    setBusy(action); setActionError("");
    try { await onFileAction(profile.id, entry, action); }
    catch (error) { setActionError(errorMessage(error)); }
    finally { setBusy(null); }
  }
  function trapFocus(event: KeyboardEvent<HTMLElement>) {
    if (event.key === "Escape") { event.preventDefault(); onClose(); }
    if (event.key !== "Tab") return;
    const controls = Array.from(drawer.current?.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], summary, [tabindex="0"]') ?? []);
    const first = controls[0];
    const last = controls.at(-1);
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
  }
  return <>
    <button type="button" className="absolute inset-0 z-20 bg-foreground/5" onClick={onClose} aria-label={t("关闭文件详情")} tabIndex={-1} />
    <aside ref={drawer} role="dialog" aria-modal="true" aria-labelledby="file-details-title" onKeyDown={trapFocus} className="absolute inset-y-0 right-0 z-30 flex w-[330px] max-w-full flex-col border-l bg-[var(--inspector)] shadow-xl">
      <div className="flex min-h-11 shrink-0 items-center justify-between border-b px-4"><h3 id="file-details-title" className="text-xs font-semibold">{t("文件详情")}</h3><Button ref={close} variant="ghost" size="icon-sm" aria-label={t("关闭详情")} onClick={onClose}><X /></Button></div>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
        <p className="mb-2 break-all text-[13px] font-semibold">{entry.name}</p><EntryStatus entry={entry} />
        {(entry.error || task?.error) && <div className="mt-4 rounded-md border border-destructive/25 bg-destructive/5 p-3 text-[11px] leading-5 text-destructive"><p className="font-medium">{entry.error ? t("无法读取文件") : (task?.errorTitle ? translate(task.errorTitle) : undefined) ?? t("转换失败")}</p><p className="mt-1 break-all">{entry.error ?? (task?.errorSuggestion ? translate(task.errorSuggestion) : task?.error)}</p>{!entry.error && task?.errorSuggestion && <details className="mt-2"><summary className="cursor-pointer">{t("技术详情")}</summary><p className="mt-1 break-all">{task.error}</p></details>}</div>}
        <dl className="mt-5 space-y-3.5 text-[11px]">
          <div><dt className="mb-1 text-muted-foreground">{t("源文件")}</dt><dd className="break-all leading-5">{path}</dd></div>
          <div><dt className="mb-1 text-muted-foreground">{t("大小 / 修改时间")}</dt><dd>{readableSize(entry.size)} · {entry.modifiedMs ? new Date(entry.modifiedMs).toLocaleString(getLocale(), { hour12: false }) : "—"}</dd></div>
          {task && <><div><dt className="mb-1 text-muted-foreground">{t("转换方式")}</dt><dd>{conversionEngineLabel(task)}</dd></div><div><dt className="mb-1 text-muted-foreground">{t("任务更新")}</dt><dd>{formatUpdatedAt(task.updatedAt)}</dd></div></>}
          <div><dt className="mb-1 text-muted-foreground">{t("分类状态")}</dt><dd>{task?.tagStatus ? translate(translate(tagStatusMeta[task.tagStatus].label)) : t("尚未分类")}</dd></div>
          <div><dt className="mb-1 text-muted-foreground">{t("输出文件")}</dt><dd className="break-all leading-5">{task?.outputPath ?? (entry.availability === "eligible" ? t("转换完成后生成 Markdown") : "—")}</dd></div>
          {task?.mineruTotalPages != null && <div><dt className="mb-1 text-muted-foreground">{t("解析进度")}</dt><dd>{task.mineruExtractedPages ?? 0} / {task.mineruTotalPages} {t("页")}</dd></div>}
        </dl>
        {(entry.sourceChanged || (task?.status === "completed" && !entry.outputAvailable)) && <div className="mt-4 rounded-md border border-amber-500/25 bg-amber-500/5 p-3 text-[11px] leading-5 text-amber-700 dark:text-amber-300">{entry.sourceChanged && <p>{t("源文件已变化，当前结果对应之前的版本。")}</p>}{task?.status === "completed" && !entry.outputAvailable && <p>{t("结果缺失：记录中的输出文件不存在或无法访问。")}</p>}</div>}
        {entry.availability === "disabled" && <p className="mt-4 text-[11px] leading-5 text-muted-foreground">{t("此格式未启用，可在“格式说明”中开启。已有任务状态保留显示。")}</p>}
        {entry.kind === "link" && <p className="mt-4 text-[11px] leading-5 text-muted-foreground">{t("链接不展开，也不会作为源文件转换。")}</p>}
      </div>
      <div className="shrink-0 border-t p-4">
        <div className="grid grid-cols-2 gap-2">
          <span title={convertBlocked || t("优先转换此文件，结束后继续其他转换任务")}><Button className="w-full" size="sm" disabled={!!busy || !!convertBlocked} onClick={() => void runAction("convert")}>{busy === "convert" ? <LoaderCircle className="animate-spin" /> : <Play />}{convertLabel}</Button></span>
          <span title={classifyBlocked || t("优先分类此文件，结束后继续其他分类任务")}><Button className="w-full" variant="outline" size="sm" disabled={!!busy || !!classifyBlocked} onClick={() => void runAction("classify")}>{busy === "classify" ? <LoaderCircle className="animate-spin" /> : <Tags />}{classifyLabel}</Button></span>
        </div>
        <p className="mt-2 text-[11px] leading-5 text-muted-foreground">{t("优先处理此文件，结束后继续同类任务。")}{(conversionPaused || classificationPaused) && t("点击会恢复对应队列。")}</p>
        {(convertBlocked || classifyBlocked) && <p className="mt-1 text-[11px] leading-5 text-muted-foreground">{convertBlocked || classifyBlocked}</p>}
        {actionError && <p role="alert" className="mt-2 break-words text-[11px] text-destructive">{actionError}</p>}
        <div className="mt-3 flex flex-wrap gap-2"><Button variant="outline" size="sm" disabled={!canOpen} onClick={() => onOpen(path)}><FileText />{t("原文件")}</Button><Button variant="outline" size="sm" onClick={() => onOpen(absoluteFilePath(profile.inputDir, parentPath(entry.relativePath)))}><FolderOpen />{t("文件夹")}</Button><Button variant="outline" size="sm" disabled={!entry.outputAvailable || !task?.outputPath} onClick={() => task?.outputPath && onOpen(task.outputPath)}><ArrowUpRight />{t("结果")}</Button></div>
      </div>
    </aside>
  </>;
}

export function DirectoryFiles({ profile, enabledExtensions, tasks, conversionPaused, monitoringPaused, classificationPaused, agentConfigured, onOpen, onFileAction }: Props) {
  const sessionKey = `${profile.id}:${profile.inputDir}:${profile.outputDir}`;
  const initial = sessions.get(sessionKey);
  const [expanded, setExpanded] = useState(() => new Set(initial?.expanded ?? []));
  const [query, setQuery] = useState(initial?.query ?? "");
  const [filter, setFilter] = useState<FileFilter>(initial?.filter ?? "all");
  const [debouncedQuery, setDebouncedQuery] = useState(query);
  const [data, setData] = useState<FileBrowserResult>(EMPTY);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState("");
  const [refreshVersion, setRefreshVersion] = useState(0);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [focusedPath, setFocusedPath] = useState<string | null>(null);
  const [scrollTop, setScrollTop] = useState(initial?.scrollTop ?? 0);
  const [viewportHeight, setViewportHeight] = useState(400);
  const [searchExpanded, setSearchExpanded] = useState(new Set<string>());
  const viewport = useRef<HTMLDivElement>(null);
  const rowElements = useRef(new Map<string, HTMLButtonElement>());
  const focusRequested = useRef(false);
  const dataRef = useRef(data);
  const visiblePathsRef = useRef<string[]>([]);
  const selectedRef = useRef(selectedPath);
  const tasksRef = useRef(tasks);
  const restoreScroll = useRef(initial?.scrollTop ?? 0);
  dataRef.current = data; selectedRef.current = selectedPath; tasksRef.current = tasks;
  const isSearch = Boolean(debouncedQuery.trim()) || filter !== "all";
  const expandedKey = JSON.stringify(expandedDirectories(expanded).sort());
  const extensionKey = JSON.stringify(enabledExtensions);
  const profileKey = JSON.stringify([profile.id, profile.inputDir, profile.outputDir]);
  const effectiveExpanded = isSearch ? searchExpanded : expanded;
  const rows = useMemo(() => flattenEntries(data.entries, effectiveExpanded, data.directories, isSearch), [data, effectiveExpanded, isSearch, getLocale()]);
  const maxStart = Math.max(0, rows.length - 1);
  const start = Math.min(maxStart, Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - 8));
  const end = Math.min(rows.length, Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + 8);
  const visibleRows = rows.slice(start, Math.max(start + 1, end));
  visiblePathsRef.current = visibleRows.flatMap((row) => row.entry?.kind !== "directory" && row.entry ? [row.path] : []);
  const selected = selectedPath ? data.entries.find((entry) => entry.relativePath === selectedPath) : undefined;
  const focusablePath = rows.some((row) => row.entry && row.path === focusedPath) ? focusedPath : rows.find((row) => row.entry)?.path;

  useEffect(() => { const timer = window.setTimeout(() => setDebouncedQuery(query), 300); return () => window.clearTimeout(timer); }, [query]);
  useEffect(() => {
    sessions.set(sessionKey, { expanded: [...expanded], query, filter, scrollTop });
  }, [sessionKey, expanded, query, filter, scrollTop]);
  useEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const observer = new ResizeObserver(() => setViewportHeight(element.clientHeight));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (loading || !viewport.current) return;
    if (restoreScroll.current) {
      viewport.current.scrollTop = restoreScroll.current;
      restoreScroll.current = 0;
    }
    setScrollTop(viewport.current.scrollTop);
  }, [loading, rows.length]);
  useEffect(() => {
    if (!loading && selectedPath && !selected) setSelectedPath(null);
  }, [loading, selectedPath, selected]);
  useEffect(() => {
    if (focusRequested.current && focusedPath && rowElements.current.has(focusedPath)) {
      rowElements.current.get(focusedPath)?.focus({ preventScroll: true });
      focusRequested.current = false;
    }
  }, [focusedPath, visibleRows]);

  useEffect(() => {
    let cancelled = false;
    let busy = false;
    let timer: number | undefined;
    const directories: string[] = JSON.parse(expandedKey);
    async function list(paths: string[], files: string[]) {
      if (!previewMode) return invoke<FileBrowserResult>("list_profile_files", { profileId: profile.id, directories: paths, files });
      const all = previewFileEntries(profile, tasksRef.current, enabledExtensions);
      return { entries: all.filter((entry) => paths.includes(parentPath(entry.relativePath)) || files.includes(entry.relativePath)), directories: paths.map((relativePath) => ({ relativePath, error: null })), missingPaths: files.filter((path) => !all.some((entry) => entry.relativePath === path)), matchedCount: 0 };
    }
    async function read(initialRead: boolean) {
      if (cancelled || busy) return;
      busy = true;
      if (initialRead) { setLoading(true); setError(""); }
      else setRefreshing(true);
      try {
        if (isSearch && initialRead) {
          const search = async () => {
            if (cancelled) return null;
            return previewMode ? previewSearch(previewFileEntries(profile, tasksRef.current, enabledExtensions), debouncedQuery, filter)
              : invoke<FileBrowserResult>("search_profile_files", { profileId: profile.id, query: debouncedQuery, filter });
          };
          const request = searchQueue.catch(() => {}).then(search);
          searchQueue = request;
          const result = await request;
          if (cancelled || !result) return;
          setData(result);
          setSearchExpanded(new Set(result.entries.filter((entry) => entry.kind === "directory").map((entry) => entry.relativePath)));
        } else if (isSearch) {
          const files = [...new Set([...visiblePathsRef.current, ...(selectedRef.current ? [selectedRef.current] : [])])];
          if (files.length) {
            const updates = await list([], files);
            if (cancelled) return;
            const updated = new Map(updates.entries.map((entry) => [entry.relativePath, entry]));
            const missing = new Set(updates.missingPaths);
            setData((current) => ({ ...current, entries: current.entries.filter((entry) => !missing.has(entry.relativePath)).map((entry) => updated.get(entry.relativePath) ?? entry), matchedCount: Math.max(0, current.matchedCount - missing.size) }));
          }
        } else {
          const result = await list(directories, selectedRef.current ? [selectedRef.current] : []);
          if (cancelled) return;
          setData(result);
        }
        if (!cancelled) setError("");
      } catch (error) {
        if (!cancelled) setError(errorMessage(error));
      } finally {
        busy = false;
        if (!cancelled) {
          setLoading(false); setRefreshing(false);
          const active = dataRef.current.entries.some((entry) => entry.task && activeStatuses.includes(entry.task.status));
          timer = window.setTimeout(() => { if (document.visibilityState === "visible") void read(false); else scheduleHidden(); }, active ? 2000 : 8000);
        }
      }
    }
    function scheduleHidden() { timer = window.setTimeout(() => { if (document.visibilityState === "visible") void read(false); else scheduleHidden(); }, 8000); }
    function onVisibility() { if (document.visibilityState === "visible" && !busy) { window.clearTimeout(timer); void read(false); } }
    void read(true);
    document.addEventListener("visibilitychange", onVisibility);
    return () => { cancelled = true; window.clearTimeout(timer); document.removeEventListener("visibilitychange", onVisibility); };
  }, [profileKey, extensionKey, isSearch ? "search" : expandedKey, debouncedQuery, filter, refreshVersion]);

  function resetScroll() { restoreScroll.current = 0; if (viewport.current) viewport.current.scrollTop = 0; setScrollTop(0); }
  function toggleDirectory(path: string) {
    const update = (current: Set<string>) => { const next = new Set(current); if (next.has(path)) next.delete(path); else next.add(path); return next; };
    if (isSearch) setSearchExpanded(update); else setExpanded(update);
  }
  function focusRow(path: string) {
    const index = rows.findIndex((row) => row.entry && row.path === path);
    if (index < 0) return;
    focusRequested.current = true;
    setFocusedPath(path);
    if (viewport.current) {
      const top = index * ROW_HEIGHT;
      if (top < viewport.current.scrollTop) viewport.current.scrollTop = top;
      else if (top + ROW_HEIGHT > viewport.current.scrollTop + viewport.current.clientHeight) viewport.current.scrollTop = top + ROW_HEIGHT - viewport.current.clientHeight;
      setScrollTop(viewport.current.scrollTop);
      rowElements.current.get(path)?.focus({ preventScroll: true });
    }
  }
  function onRowKey(event: KeyboardEvent<HTMLButtonElement>, entry: FileEntry) {
    const items = rows.filter((row) => row.entry);
    const index = items.findIndex((row) => row.path === entry.relativePath);
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      const target = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : Math.max(0, Math.min(items.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)));
      if (items[target]) focusRow(items[target].path);
    } else if (event.key === "ArrowRight" && entry.kind === "directory") {
      event.preventDefault();
      if (!effectiveExpanded.has(entry.relativePath)) toggleDirectory(entry.relativePath);
      else if (items[index + 1] && parentPath(items[index + 1].path) === entry.relativePath) focusRow(items[index + 1].path);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      if (entry.kind === "directory" && effectiveExpanded.has(entry.relativePath)) toggleDirectory(entry.relativePath);
      else focusRow(parentPath(entry.relativePath));
    }
  }
  function closeDetails() { const path = selectedPath; setSelectedPath(null); if (path) window.requestAnimationFrame(() => focusRow(path)); }
  const issues = data.directories.filter((directory) => directory.error);

  return <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-background">
    <div className="flex shrink-0 flex-wrap items-center gap-2 px-4 py-3">
      <div className="relative min-w-[130px] flex-1"><Search className="pointer-events-none absolute left-2.5 top-2.5 size-3.5 text-muted-foreground" /><Input type="search" aria-label={t("搜索此目录下的文件")} className="pl-8" placeholder={t("搜索此目录下的文件…")} value={query} onChange={(event) => { setQuery(event.target.value); resetScroll(); }} /></div>
      <select aria-label={t("文件转换状态")} value={filter} onChange={(event) => { setFilter(event.target.value as FileFilter); resetScroll(); }} className="h-8 max-w-[125px] rounded-md border border-input bg-card px-2 text-[11px] outline-none focus-visible:ring-2 focus-visible:ring-ring">{fileFilters.map((item) => <option value={item.value} key={item.value}>{translate(item.label)}</option>)}</select>
      <IconAction label={t("全部折叠")} variant="outline" onClick={() => { if (isSearch) setSearchExpanded(new Set()); else setExpanded(new Set()); resetScroll(); }}><FoldVertical /></IconAction>
      <IconAction label={t("刷新文件")} variant="outline" disabled={loading || refreshing} onClick={() => setRefreshVersion((version) => version + 1)}><RefreshCw className={cn((loading || refreshing) && "animate-spin")} /></IconAction>
    </div>
    {(!profile.enabled || monitoringPaused) && <p className="shrink-0 border-b px-4 pb-2 text-[11px] text-muted-foreground">{!profile.enabled ? t("该目录已停用") : t("目录监听已停止")}{t("，仍可浏览文件；新文件不会自动加入任务。")}</p>}
    {error && <div role="alert" className="shrink-0 border-b bg-destructive/5 px-4 py-2 text-[11px] text-destructive">{error} <button type="button" className="underline" onClick={() => setRefreshVersion((version) => version + 1)}>{t("重新读取")}</button></div>}
    {issues.length > 0 && <details className="shrink-0 border-b px-4 py-2 text-[11px] text-destructive"><summary className="cursor-pointer">{issues.length} {t("个文件夹无法读取")}</summary><ul className="mt-1 max-h-24 overflow-y-auto">{issues.map((issue) => <li className="break-all" key={issue.relativePath}>{issue.relativePath || profile.inputDir}：{issue.error}</li>)}</ul></details>}
    <div className="grid shrink-0 grid-cols-[minmax(0,1fr)_120px] border-y bg-[var(--table-head)] px-4 py-2 text-[11px] text-muted-foreground"><span>{t("名称")}</span><span>{t("转换状态")}</span></div>
    <div ref={viewport} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)} className="min-h-0 flex-1 overflow-auto" role="tree" aria-label={t("{p0}文件树", { p0: profile.name })} aria-busy={loading}>
      {loading && data.entries.length === 0 ? <div className="flex h-40 items-center justify-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-4 animate-spin" />{isSearch ? t("正在搜索全部子目录…") : t("正在读取目录…")}</div> : rows.length === 0 ? <div className="px-5 py-14 text-center text-xs text-muted-foreground">{error || issues.length ? t("目录暂时无法读取") : isSearch ? t("没有匹配的文件") : t("这是一个空文件夹")}</div> : <div className={cn("relative min-w-0", loading && "opacity-60")} style={{ height: rows.length * ROW_HEIGHT }}>
        {visibleRows.map((row, offset) => {
          const entry = row.entry;
          const style = { top: (start + offset) * ROW_HEIGHT, height: ROW_HEIGHT };
          if (!entry) return <div key={row.key} className={cn("absolute left-0 right-0 flex items-center truncate pr-3 text-[11px]", row.error ? "text-destructive" : "text-muted-foreground")} style={{ ...style, paddingLeft: 16 + (row.depth - 1) * 16 }} title={row.message}>{row.message}</div>;
          const directory = entry.kind === "directory";
          const opened = directory && effectiveExpanded.has(entry.relativePath);
          const Icon = directory ? opened ? FolderOpen : Folder : entry.kind === "link" ? Link2 : FileText;
          const focusable = focusablePath === row.path;
          return <button type="button" role="treeitem" aria-level={row.depth} aria-expanded={directory ? opened : undefined} aria-selected={selectedPath === row.path} key={row.key} ref={(element) => { if (element) rowElements.current.set(row.path, element); else rowElements.current.delete(row.path); }} tabIndex={focusable ? 0 : -1} onFocus={() => setFocusedPath(row.path)} onKeyDown={(event) => onRowKey(event, entry)} onClick={() => directory ? toggleDirectory(row.path) : setSelectedPath(row.path)} className={cn("absolute left-0 right-0 grid w-full grid-cols-[minmax(0,1fr)_120px] items-center border-b border-border/35 px-4 text-left outline-none hover:bg-accent/70 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring", selectedPath === row.path && "bg-[var(--selection)]")} style={style}>
            <span className="flex min-w-0 items-center gap-1.5 pr-2" style={{ paddingLeft: (row.depth - 1) * 16 }}><span className="flex w-3 shrink-0">{directory && (opened ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />)}</span><Icon className={cn("size-3.5 shrink-0", directory ? "text-primary" : "text-muted-foreground")} /><span className={cn("truncate text-[11px]", directory && "font-medium")} title={entry.name}>{entry.name}</span>{(entry.sourceChanged || (entry.task?.status === "completed" && !entry.outputAvailable)) && <AlertCircle className="size-3 shrink-0 text-amber-600 dark:text-amber-400" aria-label={entry.sourceChanged ? t("源文件已变化") : t("结果缺失")} />}</span><EntryStatus entry={entry} />
          </button>;
        })}
      </div>}
    </div>
    <div className="flex shrink-0 flex-wrap items-center justify-between gap-1 border-t px-4 py-2 text-[11px] text-muted-foreground" aria-live="polite"><span>{loading ? isSearch ? t("正在搜索全部子目录…") : t("正在读取…") : isSearch ? t("{p0} 个匹配文件", { p0: data.matchedCount.toLocaleString(getLocale()) }) : t("已加载 {p0} 个文件", { p0: data.entries.filter((entry) => entry.kind !== "directory").length.toLocaleString(getLocale()) })}</span><span>{isSearch ? t("刷新可更新搜索范围") : t("展开文件夹以加载子项")}</span></div>
    {selected && <FileDetails entry={selected} profile={profile} conversionPaused={conversionPaused} classificationPaused={classificationPaused} agentConfigured={agentConfigured} onOpen={onOpen} onFileAction={async (profileId, entry, action) => { await onFileAction(profileId, entry, action); setRefreshVersion((version) => version + 1); }} onClose={closeDetails} />}
  </div>;
}
