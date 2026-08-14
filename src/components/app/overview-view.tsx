import { AlertCircle, Check, ChevronRight, CircleDashed, FileOutput, FileText, FolderOpen, RadioTower, Settings2, Tags } from "lucide-react";

import { activeStatuses, formatUpdatedAt, isMarkdownTask, pendingStatuses, profileIsPersisted, taskFileName, type DirectorySaveState } from "@/app-model";
import { TaskStatus } from "@/components/app/task-status";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { TagJobRecord, TaskRecord, WatchProfile } from "@/types";

type OverviewViewProps = {
  tasks: TaskRecord[];
  tagJobs: TagJobRecord[];
  taskTotal: number;
  profiles: WatchProfile[];
  persistedProfiles: WatchProfile[];
  saveState: DirectorySaveState;
  monitoringPaused: boolean;
  conversionPaused: boolean;
  classificationPaused: boolean;
  agentConfigured: boolean;
  onOpenTasks: (taskId?: string) => void;
  onOpenDirectories: () => void;
  onOpenClassification: () => void;
};

export function OverviewView({ tasks, tagJobs, taskTotal, profiles, persistedProfiles, saveState, monitoringPaused, conversionPaused, classificationPaused, agentConfigured, onOpenTasks, onOpenDirectories, onOpenClassification }: OverviewViewProps) {
  const enabledProfiles = persistedProfiles.filter((profile) => profile.enabled);
  const classificationProfiles = enabledProfiles.filter((profile) => profile.tagging.enabled && profile.tagging.labels.length > 0);
  const pendingConversion = tasks.filter((task) => pendingStatuses.includes(task.status)).length;
  const pendingClassification = tagJobs.filter((job) => ["queued", "failed", "outdated", "cancelled"].includes(job.status)).length;
  const services = [
    {
      label: "目录监听",
      status: monitoringPaused ? "已停止" : enabledProfiles.length > 0 ? "运行中" : "等待目录",
      detail: enabledProfiles.length > 0 ? `${enabledProfiles.length} 个启用目录` : "尚未添加启用目录",
      icon: RadioTower,
      active: !monitoringPaused && enabledProfiles.length > 0,
      warning: false,
      onClick: onOpenDirectories,
    },
    {
      label: "格式转换",
      status: conversionPaused ? "已停止" : "运行中",
      detail: `${pendingConversion} 个待转换`,
      icon: FileOutput,
      active: !conversionPaused,
      warning: false,
      onClick: () => onOpenTasks(),
    },
    {
      label: "标签分类",
      status: classificationPaused
        ? "已停止"
        : classificationProfiles.length === 0
          ? "等待规则"
          : agentConfigured
            ? "运行中"
            : "等待模型",
      detail: classificationProfiles.length > 0 ? `${pendingClassification} 个待分类` : "尚未启用分类目录",
      icon: Tags,
      active: !classificationPaused && agentConfigured && classificationProfiles.length > 0,
      warning: !classificationPaused && (!agentConfigured || classificationProfiles.length === 0),
      onClick: onOpenClassification,
    },
  ];
  const stats = [
    { label: "全部转换", value: taskTotal, icon: FileText, tone: "text-foreground" },
    { label: "待执行", value: tasks.filter((task) => pendingStatuses.includes(task.status)).length, icon: CircleDashed, tone: "text-muted-foreground" },
    { label: "进行中", value: tasks.filter((task) => activeStatuses.includes(task.status)).length, icon: CircleDashed, tone: "text-foreground" },
    { label: "已完成", value: tasks.filter((task) => task.status === "completed").length, icon: Check, tone: "text-success" },
    { label: "失败", value: tasks.filter((task) => task.status === "failed").length, icon: AlertCircle, tone: "text-destructive" },
  ];

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div className="flex min-h-[62px] shrink-0 items-center border-b px-5 max-[900px]:px-4">
        <div>
          <h1 className="text-[15px] font-semibold tracking-[-0.01em]">概览</h1>
          <p className="mt-0.5 text-[11px] text-muted-foreground">监听、格式转换与标签分类的实时状态</p>
        </div>
      </div>

      <section className="grid shrink-0 grid-cols-3 divide-x border-b max-[720px]:grid-cols-1 max-[720px]:divide-x-0 max-[720px]:divide-y">
        {services.map(({ label, status, detail, icon: Icon, active, warning, onClick }) => (
          <button
            type="button"
            key={label}
            onClick={onClick}
            className="group flex h-[58px] min-w-0 items-center gap-3 px-5 text-left outline-none hover:bg-accent/60 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring max-[900px]:px-3"
          >
            <Icon className={cn("size-4 shrink-0", active ? "text-success" : warning ? "text-amber-600 dark:text-amber-400" : "text-muted-foreground")} />
            <span className="min-w-0 flex-1">
              <span className="flex min-w-0 items-center gap-2">
                <span className="truncate text-xs font-semibold">{label}</span>
                <span className={cn("inline-flex shrink-0 items-center gap-1 text-[10px] font-medium", active ? "text-success" : warning ? "text-amber-600 dark:text-amber-400" : "text-muted-foreground")}>
                  <span className={cn("size-1.5 rounded-full bg-current", active && "status-pulse")} />
                  {status}
                </span>
              </span>
              <span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{detail}</span>
            </span>
            <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/50 transition-transform group-hover:translate-x-0.5 group-hover:text-foreground" />
          </button>
        ))}
      </section>

      <section className="grid shrink-0 grid-cols-5 divide-x divide-border/70 border-b bg-background">
        {stats.map(({ label, value, icon: Icon, tone }) => (
          <div className="flex h-13 min-w-0 items-center gap-2.5 px-5 max-[900px]:px-3" key={label}>
            <Icon className={cn("size-3.5 shrink-0", tone)} />
            <div className="min-w-0">
              <p className="truncate text-[10px] text-muted-foreground">{label}</p>
              <p className="mt-0.5 text-sm font-semibold tabular-nums">{value}</p>
            </div>
          </div>
        ))}
      </section>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1.4fr)_minmax(250px,.8fr)] overflow-auto max-[860px]:grid-cols-1">
          <section className="min-w-0 border-r max-[860px]:border-b max-[860px]:border-r-0">
            <div className="flex h-11 items-center border-b px-4">
              <h2 className="text-xs font-semibold">最近活动</h2>
              <button type="button" className="ml-auto text-[10px] text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" onClick={() => onOpenTasks()}>查看全部</button>
            </div>
            {tasks.length === 0 ? (
              <div className="flex h-48 flex-col items-center justify-center px-6 text-center">
                <FileText className="mb-3 size-5 text-muted-foreground/50" />
                <p className="text-xs font-medium">暂无转换记录</p>
                <p className="mt-1 text-[11px] text-muted-foreground">监听发现文档后会先加入待执行，开始转换后再生成 Markdown。</p>
              </div>
            ) : tasks.slice(0, 6).map((task) => (
              <button key={task.id} type="button" onClick={() => onOpenTasks(task.id)} className="grid h-[48px] w-full grid-cols-[minmax(0,1fr)_86px_98px] items-center border-b px-4 text-left outline-none last:border-b-0 hover:bg-accent/70 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
                <span className="min-w-0">
                  <span className="block truncate text-xs font-medium">{taskFileName(task)}</span>
                  <span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{isMarkdownTask(task) ? "Markdown 直通" : task.engine === "mineru" ? "MinerU" : "AnyToMD"}</span>
                </span>
                <span className="text-[10px] tabular-nums text-muted-foreground">{formatUpdatedAt(task.updatedAt).split(" ").at(-1)}</span>
                <TaskStatus status={task.status} />
              </button>
            ))}
          </section>

          <section className="min-w-0">
            <div className="flex h-11 items-center border-b px-4">
              <h2 className="text-xs font-semibold">监控目录</h2>
              <Button variant="ghost" size="sm" className="ml-auto h-6 px-1.5 text-[10px]" onClick={onOpenDirectories}><Settings2 />管理</Button>
            </div>
            <div className="p-2">
              {profiles.length === 0 ? (
                <div className="px-3 py-8 text-center">
                  <p className="text-[11px] text-muted-foreground">尚未添加监控目录</p>
                  <Button className="mt-3" size="sm" onClick={onOpenDirectories}><FolderOpen />添加第一个目录</Button>
                </div>
              ) : profiles.map((profile) => {
                const persisted = profileIsPersisted(profile, persistedProfiles);
                return (
                <button key={profile.id} type="button" onClick={onOpenDirectories} className="flex h-11 w-full items-center gap-2.5 rounded-md px-2.5 text-left outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring">
                  <span className={cn("size-1.5 shrink-0 rounded-full", !persisted ? "bg-amber-500" : profile.enabled ? "bg-success" : "bg-muted-foreground/45")} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-xs font-medium">{profile.name}</span>
                    <span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{!persisted ? "待保存 · " : ""}{profile.outputDir || "尚未设置输出目录"}</span>
                  </span>
                  {!persisted && <span className="shrink-0 text-[9px] text-amber-600 dark:text-amber-400">待保存</span>}
                </button>
              );})}
              {saveState === "error" && <p className="px-3 py-2 text-[10px] text-destructive">目录设置保存失败，请进入“管理”重试。</p>}
            </div>
          </section>
      </div>
    </div>
  );
}
