import { useMemo } from "react";
import {
  ArrowUpRight,
  FileText,
  Folder,
  LoaderCircle,
  Pause,
  Play,
  RefreshCw,
  RotateCcw,
  ScanSearch,
  X,
} from "lucide-react";

import { activeStatuses, formatUpdatedAt, isMarkdownTask, pendingStatuses, statusMeta, tagStatusMeta, taskDirectory, taskFileName, type TaskFilter } from "@/app-model";
import { IconAction } from "@/components/app/icon-action";
import { TaskProgress, TaskStatus } from "@/components/app/task-status";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { TagJobStatus, TaskRecord, WatchProfile } from "@/types";

function TagStatus({ status }: { status?: TagJobStatus }) {
  if (!status) return <span className="text-[10px] text-muted-foreground">未启用</span>;
  const meta = tagStatusMeta[status];
  return <span className={cn("inline-flex items-center gap-1 text-[10px]", meta.tone === "active" && "text-primary", meta.tone === "success" && "text-success", meta.tone === "danger" && "text-destructive", meta.tone === "neutral" && "text-muted-foreground")}><span className="size-1.5 rounded-full bg-current" />{meta.label}</span>;
}

const filters: { id: TaskFilter; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "pending", label: "待执行" },
  { id: "active", label: "进行中" },
  { id: "completed", label: "已完成" },
  { id: "failed", label: "失败" },
];

type TaskWorkspaceProps = {
  tasks: TaskRecord[];
  total: number;
  profiles: WatchProfile[];
  filter: TaskFilter;
  onFilterChange: (filter: TaskFilter) => void;
  query: string;
  selectedTaskId: string | null;
  onSelectTask: (taskId: string) => void;
  inspectorOpen: boolean;
  onCloseInspector: () => void;
  retryingIds: Set<string>;
  onRetry: (task: TaskRecord) => void;
  onOpenResult: (path: string) => void;
  onRefresh: () => void;
  onRescan: () => void;
  rescanning: boolean;
  onRetryFailed: () => void;
  retryingFailed: boolean;
  refreshing: boolean;
  loading: boolean;
  paused: boolean;
  pausing: boolean;
  onTogglePaused: () => void;
};

function filterTask(task: TaskRecord, filter: TaskFilter) {
  if (filter === "pending") return pendingStatuses.includes(task.status);
  if (filter === "active") return activeStatuses.includes(task.status);
  if (filter === "completed") return task.status === "completed";
  if (filter === "failed") return task.status === "failed";
  return true;
}

function TaskInspector({
  task,
  profile,
  retrying,
  onRetry,
  onOpenResult,
  onClose,
}: {
  task: TaskRecord | null;
  profile?: WatchProfile;
  retrying: boolean;
  onRetry: (task: TaskRecord) => void;
  onOpenResult: (path: string) => void;
  onClose?: () => void;
}) {
  if (!task) {
    return (
      <div className="flex h-full flex-col items-center justify-center px-7 text-center text-muted-foreground">
        <FileText className="mb-3 size-5 opacity-55" />
        <p className="text-xs font-medium text-foreground">选择一项任务</p>
        <p className="mt-1 text-[11px] leading-5">查看转换引擎、路径与输出结果。</p>
      </div>
    );
  }

  const active = activeStatuses.includes(task.status);
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-11 items-center border-b px-4">
        <span className="text-xs font-semibold">任务详情</span>
        {onClose && (
          <div className="ml-auto">
            <IconAction label="关闭详情" size="icon-sm" onClick={onClose}><X /></IconAction>
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
        <div className="mb-5 flex items-start gap-2.5">
          <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md border bg-card text-muted-foreground shadow-xs">
            <FileText className="size-4" />
          </div>
          <div className="min-w-0">
            <p className="break-words text-[13px] font-semibold leading-5">{taskFileName(task)}</p>
            <div className="mt-1"><TaskStatus status={task.status} /></div>
          </div>
        </div>

        <dl className="space-y-3.5 text-[11px]">
          <div>
            <dt className="mb-1 text-muted-foreground">转换引擎</dt>
            <dd className="text-foreground">{isMarkdownTask(task) ? "Markdown 直通同步" : task.engine === "mineru" ? "MinerU 文档解析" : "AnyToMD 本地转换"}</dd>
          </div>
          <div>
            <dt className="mb-1 text-muted-foreground">所属目录</dt>
            <dd className="truncate text-foreground" title={profile?.name}>{profile?.name ?? "未知目录"}</dd>
          </div>
          <div>
            <dt className="mb-1 text-muted-foreground">源文件</dt>
            <dd className="break-all leading-4 text-foreground" title={task.sourcePath}>{task.sourcePath}</dd>
          </div>
          <div>
            <dt className="mb-1 text-muted-foreground">更新时间</dt>
            <dd className="text-foreground">{formatUpdatedAt(task.updatedAt)}</dd>
          </div>
          <div>
            <dt className="mb-1 text-muted-foreground">输出文件</dt>
            <dd className={cn("break-all leading-4", task.outputPath ? "text-foreground" : "text-muted-foreground")}>
              {task.outputPath ?? "转换完成后生成同名 .md 文件"}
            </dd>
          </div>
          <div><dt className="mb-1 text-muted-foreground">分类状态</dt><dd><TagStatus status={task.tagStatus} /></dd></div>
        </dl>

        {task.error && (
          <div className="mt-5 rounded-md border border-destructive/25 bg-destructive/5 px-3 py-2.5 text-[11px] leading-5 text-destructive">
            <p className="font-medium">{task.errorTitle ?? "任务执行失败"}</p>
            <p className="mt-1 text-destructive/85">{task.errorSuggestion}</p>
            <details className="mt-2"><summary className="cursor-pointer text-[10px]">技术详情</summary><p className="mt-1 break-all text-[10px] opacity-80">{task.error}</p></details>
          </div>
        )}
      </div>

      <div className="space-y-3 border-t px-4 py-4">
        {active && <TaskProgress task={task} />}
        <div className="flex gap-2">
          <Button className="min-w-0 flex-1" disabled={!task.outputPath} onClick={() => task.outputPath && onOpenResult(task.outputPath)}>
            <ArrowUpRight />打开结果
          </Button>
          {task.status === "failed" && (
            <IconAction label="重新转换" variant="outline" disabled={retrying} onClick={() => onRetry(task)}>
              {retrying ? <LoaderCircle className="animate-spin" /> : <RotateCcw />}
            </IconAction>
          )}
        </div>
      </div>
    </div>
  );
}

export function TaskWorkspace({
  tasks,
  total,
  profiles,
  filter,
  onFilterChange,
  query,
  selectedTaskId,
  onSelectTask,
  inspectorOpen,
  onCloseInspector,
  retryingIds,
  onRetry,
  onOpenResult,
  onRefresh,
  onRescan,
  rescanning,
  onRetryFailed,
  retryingFailed,
  refreshing,
  loading,
  paused,
  pausing,
  onTogglePaused,
}: TaskWorkspaceProps) {
  const taskCounts = useMemo(() => ({
    all: total,
    pending: tasks.filter((task) => pendingStatuses.includes(task.status)).length,
    active: tasks.filter((task) => activeStatuses.includes(task.status)).length,
    completed: tasks.filter((task) => task.status === "completed").length,
    failed: tasks.filter((task) => task.status === "failed").length,
  }), [tasks, total]);

  const normalizedQuery = query.trim().toLocaleLowerCase();
  const visibleTasks = useMemo(() => tasks.filter((task) => {
    if (!filterTask(task, filter)) return false;
    if (!normalizedQuery) return true;
    const haystack = [task.relativePath, task.sourcePath, task.engine, statusMeta[task.status].label].join(" ").toLocaleLowerCase();
    return haystack.includes(normalizedQuery);
  }), [filter, normalizedQuery, tasks]);

  const selectedTask = tasks.find((task) => task.id === selectedTaskId) ?? null;
  const selectedProfile = profiles.find((profile) => profile.id === selectedTask?.profileId);
  const selectedRetrying = selectedTask ? retryingIds.has(selectedTask.id) : false;

  return (
    <div className="relative grid h-full w-full min-h-0 min-w-0 max-w-full grid-cols-[minmax(0,1fr)_300px] overflow-hidden max-[1180px]:block">
      <section className="relative flex h-full w-full min-h-0 min-w-0 max-w-full flex-col overflow-hidden bg-background">
        <div className="grid min-h-[98px] w-full min-w-0 shrink-0 grid-cols-[auto_minmax(0,1fr)] grid-rows-[auto_auto] items-center gap-x-3 gap-y-1.5 border-b px-5 py-2.5 max-[900px]:px-4">
          <div className="shrink-0">
            <h1 className="text-[15px] font-semibold tracking-[-0.01em]">转换任务</h1>
            <div className="mt-0.5 flex items-center gap-2 whitespace-nowrap text-[11px] text-muted-foreground">
              <span className="max-[1250px]:hidden">{total > tasks.length ? `显示最近 ${tasks.length.toLocaleString()} / 共 ${total.toLocaleString()} 条` : "文档转换队列、目录监控与输出状态"}</span>
              <span className={cn("inline-flex shrink-0 items-center gap-1 font-medium", !paused && "text-success")}>
                <span className={cn("size-1.5 rounded-full bg-current", !paused && "status-pulse")} />
                {paused ? "转换已停止" : "转换运行中"}
              </span>
            </div>
          </div>
          <div className="col-span-2 row-start-2 flex w-fit max-w-full shrink-0 items-center gap-0.5 overflow-x-auto whitespace-nowrap rounded-md border bg-card p-0.5 shadow-xs [-ms-overflow-style:none] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            {filters.map((item) => (
              <button
                key={item.id}
                type="button"
                onClick={() => onFilterChange(item.id)}
                className={cn(
                  "h-6 rounded px-2 text-[10px] outline-none transition focus-visible:ring-2 focus-visible:ring-ring",
                  filter === item.id ? "bg-[var(--selection)] font-medium text-foreground" : "text-muted-foreground hover:bg-accent hover:text-foreground",
                )}
              >
                {item.label} <span className="ml-0.5 tabular-nums opacity-75">{taskCounts[item.id]}</span>
              </button>
            ))}
          </div>
          <div className="col-start-2 row-start-1 flex shrink-0 items-center justify-self-start gap-2">
            {taskCounts.failed > 0 && (
              <Button className="shrink-0 whitespace-nowrap" variant="outline" size="sm" disabled={retryingFailed} onClick={onRetryFailed}>
                {retryingFailed ? <LoaderCircle className="animate-spin" /> : <RotateCcw />}重试失败 {taskCounts.failed}
              </Button>
            )}
            <Button className="shrink-0 whitespace-nowrap" variant={paused ? "default" : "outline"} size="sm" disabled={pausing || loading} onClick={onTogglePaused}>
              {pausing ? <LoaderCircle className="animate-spin" /> : paused ? <Play /> : <Pause />}
              {paused ? "开始转换" : "停止转换"}
            </Button>
            <IconAction label="重新扫描监控目录" size="icon-sm" disabled={rescanning || loading} onClick={onRescan}>
              <ScanSearch className={cn(rescanning && "animate-pulse")} />
            </IconAction>
            <IconAction label="刷新转换任务" size="icon-sm" disabled={refreshing} onClick={onRefresh}>
              <RefreshCw className={cn(refreshing && "animate-spin")} />
            </IconAction>
          </div>
        </div>

        <div className="absolute inset-x-0 bottom-0 top-[98px] min-h-0 min-w-0 overflow-auto">
          <div className="min-w-[540px]">
            <div className="grid h-8 grid-cols-[minmax(210px,1fr)_116px_76px_96px_104px] items-center border-b bg-[var(--table-head)] px-4 text-[10px] font-medium uppercase tracking-[0.04em] text-muted-foreground">
              <span>文件名</span><span>来源</span><span>更新</span><span>转换状态</span><span>分类状态</span>
            </div>

            {loading ? (
              <div className="flex h-40 items-center justify-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-4 animate-spin" />正在载入任务</div>
            ) : visibleTasks.length === 0 ? (
              <div className="flex h-48 flex-col items-center justify-center px-6 text-center">
                <FileText className="mb-3 size-5 text-muted-foreground/55" />
                <p className="text-xs font-medium">{query ? "没有匹配的任务" : "这里还没有任务"}</p>
                <p className="mt-1 text-[11px] text-muted-foreground">{query ? "尝试更换关键词或状态筛选。" : "把支持的文档放入监控目录后会自动出现。"}</p>
              </div>
            ) : visibleTasks.map((task) => {
              const profile = profiles.find((item) => item.id === task.profileId);
              const selected = task.id === selectedTaskId;
              return (
                <button
                  key={task.id}
                  type="button"
                  onClick={() => onSelectTask(task.id)}
                  className={cn(
                    "grid h-[50px] w-full grid-cols-[minmax(210px,1fr)_116px_76px_96px_104px] items-center border-b px-4 text-left text-xs outline-none transition-colors focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
                    selected ? "bg-[var(--selection)]" : "hover:bg-accent/70",
                  )}
                  aria-pressed={selected}
                >
                  <span className="flex min-w-0 items-center gap-2.5">
                    <span className="flex size-7 shrink-0 items-center justify-center rounded-md border bg-card text-muted-foreground shadow-xs"><FileText className="size-3.5" /></span>
                    <span className="min-w-0">
                      <span className="block truncate font-medium" title={taskFileName(task)}>{taskFileName(task)}</span>
                      <span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{taskDirectory(task)}</span>
                    </span>
                  </span>
                  <span className="truncate pr-3 text-[11px] text-muted-foreground" title={profile?.name}><Folder className="mr-1 inline size-3 -translate-y-px" />{profile?.name ?? "未知"}</span>
                  <span className="text-[10px] tabular-nums text-muted-foreground">{formatUpdatedAt(task.updatedAt).split(" ").at(-1)}</span>
                  <TaskStatus status={task.status} />
                  <TagStatus status={task.tagStatus} />
                </button>
              );
            })}
          </div>
        </div>
      </section>

      <aside className="min-h-0 border-l bg-[var(--inspector)] max-[1180px]:hidden">
        <TaskInspector task={selectedTask} profile={selectedProfile} retrying={selectedRetrying} onRetry={onRetry} onOpenResult={onOpenResult} />
      </aside>

      {inspectorOpen && (
        <div className="fixed inset-0 z-40 hidden bg-black/20 backdrop-blur-[1px] max-[1180px]:block" onMouseDown={onCloseInspector}>
          <aside className="ml-auto h-full w-[340px] max-w-[84vw] border-l bg-[var(--inspector)] shadow-[-12px_0_36px_rgba(0,0,0,0.16)]" onMouseDown={(event) => event.stopPropagation()}>
            <TaskInspector task={selectedTask} profile={selectedProfile} retrying={selectedRetrying} onRetry={onRetry} onOpenResult={onOpenResult} onClose={onCloseInspector} />
          </aside>
        </div>
      )}
    </div>
  );
}
