import { useEffect, useMemo, useState } from "react";
import { ArrowUpRight, Bot, FileText, LoaderCircle, Pause, Play, RefreshCw, RotateCcw, X } from "lucide-react";

import { formatUpdatedAt, tagStatusMeta, type TagFilter } from "@/app-model";
import { IconAction } from "@/components/app/icon-action";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { TagJobRecord, TagJobStatus, WatchProfile } from "@/types";

const filters: { id: TagFilter; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "queued", label: "待执行" },
  { id: "active", label: "读取中" },
  { id: "completed", label: "已完成" },
  { id: "failed", label: "失败" },
  { id: "outdated", label: "已过期" },
];

function matchesFilter(status: TagJobStatus, filter: TagFilter) {
  if (filter === "active") return status === "reading" || status === "writing";
  if (filter === "all") return true;
  return status === filter;
}

function TagStatus({ status }: { status: TagJobStatus }) {
  const meta = tagStatusMeta[status];
  return (
    <span className={cn(
      "inline-flex shrink-0 items-center gap-1 whitespace-nowrap text-[10px] font-medium",
      meta.tone === "active" && "text-primary",
      meta.tone === "success" && "text-success",
      meta.tone === "danger" && "text-destructive",
      meta.tone === "neutral" && "text-muted-foreground",
    )}>
      <span className={cn("size-1.5 rounded-full bg-current", meta.tone === "active" && "status-pulse")} />
      {meta.label}
    </span>
  );
}

function parsedCategories(job: TagJobRecord) {
  if (!job.resultJson) return [] as string[];
  try {
    const value = JSON.parse(job.resultJson) as unknown;
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [] as string[];
  }
}

type TagTasksViewProps = {
  jobs: TagJobRecord[];
  total: number;
  profiles: WatchProfile[];
  filter: TagFilter;
  onFilterChange: (filter: TagFilter) => void;
  query: string;
  selectedId: string | null;
  onSelect: (id: string) => void;
  retryingIds: Set<string>;
  onRetry: (ids: string[]) => void;
  onOpen: (path: string) => void;
  onRefresh: () => void;
  refreshing: boolean;
  loading: boolean;
  classificationPaused: boolean;
  agentConfigured: boolean;
  changingClassificationState: boolean;
  onToggleClassificationPaused: () => void;
  onOpenSettings: () => void;
  inspectorOpen: boolean;
  onCloseInspector: () => void;
};

function TagInspector({ job, profile, retrying, onRetry, onOpen, onClose }: {
  job: TagJobRecord | null;
  profile?: WatchProfile;
  retrying: boolean;
  onRetry: (ids: string[]) => void;
  onOpen: (path: string) => void;
  onClose?: () => void;
}) {
  if (!job) {
    return <div className="flex h-full flex-col items-center justify-center px-7 text-center"><Bot className="mb-3 size-5 text-muted-foreground/55" /><p className="text-xs font-medium">选择一项分类任务</p></div>;
  }
  const categories = parsedCategories(job);
  const coverage = job.totalBytes > 0 ? Math.min(100, Math.round((job.readBytes / job.totalBytes) * 100)) : 0;
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-11 items-center border-b px-4 text-xs font-semibold">
        分类详情
        {onClose && <div className="ml-auto"><IconAction label="关闭详情" size="icon-sm" onClick={onClose}><X /></IconAction></div>}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
        <p className="break-words text-[13px] font-semibold leading-5">{job.relativePath.split(/[\\/]/).at(-1)}</p>
        <div className="mt-1"><TagStatus status={job.status} /></div>
        <dl className="mt-5 space-y-3.5 text-[11px]">
          <div><dt className="mb-1 text-muted-foreground">所属目录</dt><dd>{profile?.name ?? "未知目录"}</dd></div>
          <div><dt className="mb-1 text-muted-foreground">Markdown</dt><dd className="break-all leading-4">{job.markdownPath}</dd></div>
          <div><dt className="mb-1 text-muted-foreground">读取覆盖量</dt><dd>{job.readBytes > 0 ? `${coverage}%（${job.readBytes} / ${job.totalBytes} bytes）` : "尚未读取"}</dd></div>
          <div><dt className="mb-1 text-muted-foreground">模型用量</dt><dd className="font-medium tabular-nums">总计 {(job.inputTokens + job.outputTokens).toLocaleString()} Token</dd><dd className="mt-0.5 text-[10px] text-muted-foreground">{job.apiCalls} 次请求 · {job.inputTokens.toLocaleString()} 输入 / {job.outputTokens.toLocaleString()} 输出</dd></div>
        </dl>
        {categories.length > 0 && <div className="mt-5"><p className="mb-2 text-[10px] font-medium text-muted-foreground">所属类别</p><div className="flex flex-wrap gap-1.5">{categories.map((category) => <span key={category} className="rounded-full border bg-card px-2.5 py-1 text-[10px] font-medium">{category}</span>)}</div></div>}
        {job.error && <div className="mt-5 rounded-md border border-destructive/25 bg-destructive/5 px-3 py-2.5 text-[11px] leading-5 text-destructive"><p className="font-medium">{job.errorTitle ?? "分类执行失败"}</p><p className="mt-1 text-destructive/85">{job.errorSuggestion}</p><details className="mt-2"><summary className="cursor-pointer text-[10px]">技术详情</summary><p className="mt-1 break-all text-[10px] opacity-80">{job.error}</p></details></div>}
      </div>
      <div className="flex gap-2 border-t px-4 py-4">
        <Button className="min-w-0 flex-1" onClick={() => onOpen(job.markdownPath)}><ArrowUpRight />打开 Markdown</Button>
        <IconAction label="重新分类" variant="outline" disabled={retrying} onClick={() => onRetry([job.id])}>{retrying ? <LoaderCircle className="animate-spin" /> : <RotateCcw />}</IconAction>
      </div>
    </div>
  );
}

export function TagTasksView({ jobs, total, profiles, filter, onFilterChange, query, selectedId, onSelect, retryingIds, onRetry, onOpen, onRefresh, refreshing, loading, classificationPaused, agentConfigured, changingClassificationState, onToggleClassificationPaused, onOpenSettings, inspectorOpen, onCloseInspector }: TagTasksViewProps) {
  const [confirmation, setConfirmation] = useState<"start" | "process" | null>(null);
  const counts = useMemo(() => ({
    all: total,
    queued: jobs.filter((job) => job.status === "queued").length,
    active: jobs.filter((job) => job.status === "reading" || job.status === "writing").length,
    completed: jobs.filter((job) => job.status === "completed").length,
    failed: jobs.filter((job) => job.status === "failed").length,
    outdated: jobs.filter((job) => job.status === "outdated").length,
  }), [jobs, total]);
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const visible = useMemo(() => jobs.filter((job) => {
    if (!matchesFilter(job.status, filter)) return false;
    if (!normalizedQuery) return true;
    const profile = profiles.find((item) => item.id === job.profileId);
    return [job.relativePath, job.markdownPath, profile?.name, job.error]
      .join(" ")
      .toLocaleLowerCase()
      .includes(normalizedQuery);
  }), [filter, jobs, normalizedQuery, profiles]);
  const selected = jobs.find((job) => job.id === selectedId) ?? null;
  const profile = profiles.find((item) => item.id === selected?.profileId);
  const actionable = jobs.filter((job) => ["failed", "outdated", "cancelled"].includes(job.status));
  const startableCount = jobs.filter((job) => ["queued", "failed", "outdated", "cancelled"].includes(job.status)).length;
  const retryingActionable = actionable.some((job) => retryingIds.has(job.id));

  useEffect(() => setConfirmation(null), [classificationPaused, startableCount]);

  function processActionable() {
    if (confirmation !== "process") {
      setConfirmation("process");
      return;
    }
    setConfirmation(null);
    onRetry(actionable.map((job) => job.id));
  }

  function toggleClassification() {
    if (classificationPaused && startableCount > 0 && confirmation !== "start") {
      setConfirmation("start");
      return;
    }
    setConfirmation(null);
    onToggleClassificationPaused();
  }

  const runtimeLabel = classificationPaused
      ? "分类已停止"
      : !agentConfigured
        ? "等待 Agent 配置"
        : "分类运行中";
  const runtimeActive = !classificationPaused && agentConfigured;

  return (
    <div className="grid h-full w-full min-h-0 min-w-0 max-w-full grid-cols-[minmax(0,1fr)_300px] overflow-hidden max-[1180px]:block">
      <section className="relative flex h-full w-full min-h-0 min-w-0 max-w-full flex-col overflow-hidden">
        <div className="grid min-h-[98px] w-full min-w-0 shrink-0 grid-cols-[auto_minmax(0,1fr)] grid-rows-[auto_auto] items-center gap-x-3 gap-y-1.5 border-b px-5 py-2.5 max-[900px]:px-4">
          <div className="shrink-0">
            <h1 className="text-[15px] font-semibold tracking-[-0.01em]">分类任务</h1>
            <div className="mt-0.5 flex items-center gap-2 whitespace-nowrap text-[11px] text-muted-foreground">
              <span className="max-[1250px]:hidden">{total > jobs.length ? `显示最近 ${jobs.length.toLocaleString()} / 共 ${total.toLocaleString()} 条` : "Agent 从候选类别中判断文档类型，并写入 cpah_categories"}</span>
              <span className={cn("inline-flex shrink-0 items-center gap-1 font-medium", runtimeActive && "text-success", !agentConfigured && !classificationPaused && "text-amber-600 dark:text-amber-400")}>
                <span className={cn("size-1.5 rounded-full bg-current", runtimeActive && "status-pulse")} />
                {runtimeLabel}
              </span>
            </div>
          </div>
          <div className="col-span-2 row-start-2 flex w-fit max-w-full shrink-0 items-center gap-0.5 overflow-x-auto whitespace-nowrap rounded-md border bg-card p-0.5 shadow-xs [-ms-overflow-style:none] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            {filters.map((item) => (
              <button key={item.id} type="button" onClick={() => onFilterChange(item.id)} className={cn("h-6 shrink-0 whitespace-nowrap rounded px-2 text-[10px] outline-none transition focus-visible:ring-2 focus-visible:ring-ring", filter === item.id ? "bg-[var(--selection)] font-medium" : "text-muted-foreground hover:bg-accent")}>
                {item.label} <span className="ml-0.5 tabular-nums opacity-75">{counts[item.id]}</span>
              </button>
            ))}
          </div>
          <div className="col-start-2 row-start-1 flex shrink-0 items-center justify-self-start gap-2">
            {runtimeActive && actionable.length > 0 && (
              <Button className="shrink-0 whitespace-nowrap" variant={confirmation === "process" ? "default" : "outline"} size="sm" disabled={retryingActionable} onClick={processActionable}>
                {retryingActionable ? <LoaderCircle className="animate-spin" /> : <RotateCcw />}
                {confirmation === "process" ? `确认调用模型 · ${actionable.length} 篇` : `处理待分类 ${actionable.length}`}
              </Button>
            )}
            {!agentConfigured ? (
              <Button className="shrink-0 whitespace-nowrap" variant="outline" size="sm" onClick={onOpenSettings}><Bot />配置 Agent</Button>
            ) : (
              <Button
                className="shrink-0 whitespace-nowrap"
                variant={classificationPaused ? "default" : "outline"}
                size="sm"
                disabled={changingClassificationState}
                onClick={toggleClassification}
              >
                {changingClassificationState ? <LoaderCircle className="animate-spin" /> : classificationPaused ? <Play /> : <Pause />}
                {classificationPaused && confirmation === "start" ? `确认开始 · ${startableCount} 篇` : classificationPaused ? "开始分类" : "停止分类"}
              </Button>
            )}
            <IconAction label="刷新分类任务" size="icon-sm" disabled={refreshing} onClick={onRefresh}><RefreshCw className={cn(refreshing && "animate-spin")} /></IconAction>
          </div>
        </div>

        <div className="absolute inset-x-0 bottom-0 top-[98px] min-h-0 min-w-0 overflow-auto">
          <div className="min-w-[730px]">
            <div className="grid h-8 grid-cols-[minmax(230px,1fr)_120px_80px_100px_80px_100px] items-center border-b bg-[var(--table-head)] px-4 text-[10px] font-medium text-muted-foreground">
              <span>Markdown</span><span>目录</span><span>更新</span><span>读取覆盖</span><span>Token</span><span>状态</span>
            </div>
            {loading ? (
              <div className="flex h-40 items-center justify-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-4 animate-spin" />正在载入任务</div>
            ) : visible.length === 0 ? (
              <div className="flex h-48 flex-col items-center justify-center px-6 text-center"><Bot className="mb-3 size-5 text-muted-foreground/55" /><p className="text-xs font-medium">没有匹配的分类任务</p><p className="mt-1 text-[11px] text-muted-foreground">先在监控目录中开启分类并配置候选类别；新增 Markdown 会在这里出现。</p></div>
            ) : visible.map((job) => {
              const itemProfile = profiles.find((item) => item.id === job.profileId);
              const itemCoverage = job.totalBytes > 0 ? Math.min(100, Math.round((job.readBytes / job.totalBytes) * 100)) : 0;
              return (
                <button key={job.id} type="button" onClick={() => onSelect(job.id)} className={cn("grid h-[50px] w-full grid-cols-[minmax(230px,1fr)_120px_80px_100px_80px_100px] items-center border-b px-4 text-left text-xs outline-none hover:bg-accent/70 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring", selectedId === job.id && "bg-[var(--selection)]")}>
                  <span className="flex min-w-0 items-center gap-2.5"><span className="flex size-7 shrink-0 items-center justify-center rounded-md border bg-card text-muted-foreground"><FileText className="size-3.5" /></span><span className="min-w-0"><span className="block truncate font-medium">{job.relativePath.split(/[\\/]/).at(-1)}</span><span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{job.relativePath}</span></span></span>
                  <span className="truncate pr-2 text-[11px] text-muted-foreground">{itemProfile?.name ?? "未知"}</span>
                  <span className="text-[10px] tabular-nums text-muted-foreground">{formatUpdatedAt(job.updatedAt).split(" ").at(-1)}</span>
                  <span className="text-[10px] tabular-nums text-muted-foreground">{job.readBytes > 0 ? `${itemCoverage}% · ${Math.ceil(job.readBytes / 1024)} KiB` : "—"}</span>
                  <span className="text-[10px] tabular-nums text-muted-foreground">{job.inputTokens + job.outputTokens > 0 ? (job.inputTokens + job.outputTokens).toLocaleString() : "—"}</span>
                  <TagStatus status={job.status} />
                </button>
              );
            })}
          </div>
        </div>
      </section>

      <aside className="min-h-0 border-l bg-[var(--inspector)] max-[1180px]:hidden">
        <TagInspector job={selected} profile={profile} retrying={selected ? retryingIds.has(selected.id) : false} onRetry={onRetry} onOpen={onOpen} />
      </aside>

      {inspectorOpen && (
        <div className="fixed inset-0 z-40 hidden bg-black/20 backdrop-blur-[1px] max-[1180px]:block" onMouseDown={onCloseInspector}>
          <aside className="ml-auto h-full w-[340px] max-w-[84vw] border-l bg-[var(--inspector)] shadow-[-12px_0_36px_rgba(0,0,0,0.16)]" onMouseDown={(event) => event.stopPropagation()}>
            <TagInspector job={selected} profile={profile} retrying={selected ? retryingIds.has(selected.id) : false} onRetry={onRetry} onOpen={onOpen} onClose={onCloseInspector} />
          </aside>
        </div>
      )}
    </div>
  );
}
