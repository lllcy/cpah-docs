import { type RefObject, type ReactNode } from "react";
import {
  AppWindow,
  BookOpenText,
  Command,
  CircleHelp,
  Folders,
  LayoutDashboard,
  ListTodo,
  Tags,
  RefreshCw,
  Search,
  Settings2,
} from "lucide-react";

import type { View } from "@/app-model";
import { IconAction } from "@/components/app/icon-action";
import { cn } from "@/lib/utils";

const navigation = [
  { id: "overview" as const, label: "概览", icon: LayoutDashboard },
  { id: "directories" as const, label: "监控目录", icon: Folders },
  { id: "tasks" as const, label: "转换任务", icon: ListTodo },
  { id: "tagging" as const, label: "分类任务", icon: Tags },
  { id: "formats" as const, label: "格式说明", icon: BookOpenText },
  { id: "help" as const, label: "帮助", icon: CircleHelp },
  { id: "settings" as const, label: "设置", icon: Settings2 },
];

type AppShellProps = {
  activeView: View;
  onViewChange: (view: View) => void;
  taskCount: number;
  tagJobCount: number;
  pendingCount: number;
  activeCount: number;
  enabledDirectories: number;
  monitoringPaused: boolean;
  loading: boolean;
  loadError: string;
  refreshing: boolean;
  onRefresh: () => void;
  query: string;
  onQueryChange: (value: string) => void;
  commandInputRef: RefObject<HTMLInputElement | null>;
  previewNativeBar: boolean;
  children: ReactNode;
};

export function AppShell({
  activeView,
  onViewChange,
  taskCount,
  tagJobCount,
  pendingCount,
  activeCount,
  enabledDirectories,
  monitoringPaused,
  loading,
  loadError,
  refreshing,
  onRefresh,
  query,
  onQueryChange,
  commandInputRef,
  previewNativeBar,
  children,
}: AppShellProps) {
  const stateLabel = loadError ? "连接异常" : loading ? "正在连接" : monitoringPaused ? "监听已停止" : `正在监听 ${enabledDirectories} 个目录`;

  return (
    <div className="flex h-full min-h-0 flex-col bg-background text-foreground">
      {previewNativeBar && (
        <div className="flex h-8 shrink-0 items-center border-b border-border/70 bg-[var(--native)] px-3 text-[11px] text-muted-foreground">
          <AppWindow className="mr-2 size-3.5 text-primary" />
          <span>CPAH Docs</span>
          <div className="ml-auto flex h-full items-stretch text-foreground/70" aria-hidden="true">
            <span className="flex w-11 items-center justify-center">—</span>
            <span className="flex w-11 items-center justify-center text-[10px]">□</span>
            <span className="flex w-11 items-center justify-center">×</span>
          </div>
        </div>
      )}

      <header className="app-toolbar grid h-11 shrink-0 grid-cols-[184px_minmax(220px,1fr)_230px] items-center border-b bg-[var(--native)] max-[900px]:grid-cols-[152px_minmax(180px,1fr)_176px]">
        <div className="flex min-w-0 items-center gap-2 px-3.5">
          <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground shadow-xs">
            <Command className="size-3.5" />
          </span>
          <span className="truncate text-[13px] font-semibold tracking-[-0.01em] max-[900px]:hidden">CPAH Docs</span>
        </div>

        <label className="group mx-auto flex h-7 w-full max-w-[430px] items-center gap-2 rounded-md border border-input bg-card px-2.5 shadow-xs transition focus-within:border-ring focus-within:ring-2 focus-within:ring-ring/15">
          <Search className="size-3.5 shrink-0 text-muted-foreground" />
          <input
            ref={commandInputRef}
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            className="min-w-0 flex-1 bg-transparent text-xs outline-none placeholder:text-muted-foreground/80"
            placeholder="搜索任务或执行命令"
            aria-label="搜索任务"
          />
          <kbd className="shrink-0 rounded border bg-muted px-1.5 py-0.5 text-[9px] text-muted-foreground max-[820px]:hidden">Ctrl K</kbd>
        </label>

        <div className="flex items-center justify-end gap-1.5 px-3 max-[900px]:px-2">
          <div className="mr-1 flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground">
            <span className={cn("size-1.5 shrink-0 rounded-full", loadError ? "bg-destructive" : monitoringPaused ? "bg-muted-foreground" : "bg-success", !monitoringPaused && !loadError && "status-pulse")} />
            <span className="truncate max-[900px]:hidden">{stateLabel}</span>
          </div>
          <IconAction label="刷新状态" size="icon-sm" disabled={refreshing} onClick={onRefresh}>
            <RefreshCw className={cn("size-3.5", refreshing && "animate-spin")} />
          </IconAction>
        </div>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[184px_minmax(0,1fr)] max-[900px]:grid-cols-[152px_minmax(0,1fr)]">
        <aside className="flex min-h-0 flex-col border-r bg-[var(--sidebar)] px-2 py-3">
          <nav aria-label="主导航" className="space-y-0.5">
            {navigation.map(({ id, label, icon: Icon }) => {
              const active = activeView === id;
              return (
                <button
                  key={id}
                  type="button"
                  onClick={() => onViewChange(id)}
                  className={cn(
                    "flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-left text-xs outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring",
                    active ? "bg-[var(--selection)] font-medium text-foreground shadow-[inset_0_0_0_1px_color-mix(in_srgb,var(--border)_65%,transparent)]" : "text-muted-foreground hover:bg-accent hover:text-foreground",
                  )}
                  aria-current={active ? "page" : undefined}
                >
                  <Icon className="size-3.5 shrink-0" />
                  <span className="truncate">{label}</span>
                  {id === "tasks" && taskCount > 0 && <span className="ml-auto rounded px-1.5 py-0.5 text-[10px] tabular-nums text-muted-foreground">{taskCount}</span>}
                  {id === "tagging" && tagJobCount > 0 && <span className="ml-auto rounded px-1.5 py-0.5 text-[10px] tabular-nums text-muted-foreground">{tagJobCount}</span>}
                </button>
              );
            })}
          </nav>

          <div className="mt-auto border-t px-2 pt-3 text-[10px] leading-5 text-muted-foreground">
            <div className="flex items-center justify-between gap-2"><span>待执行</span><span className="tabular-nums text-foreground">{pendingCount}</span></div>
            <div className="flex items-center justify-between gap-2"><span>进行中</span><span className="tabular-nums text-foreground">{activeCount}</span></div>
            <div className="flex items-center justify-between gap-2"><span>监控目录</span><span className="tabular-nums text-foreground">{enabledDirectories}</span></div>
          </div>
        </aside>

        <main className="min-h-0 min-w-0 overflow-hidden bg-background">{children}</main>
      </div>
    </div>
  );
}
