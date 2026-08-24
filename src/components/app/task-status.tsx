import { Check, Circle, CircleAlert, LoaderCircle } from "lucide-react";

import { activeStatuses, statusMeta } from "@/app-model";
import { cn } from "@/lib/utils";
import type { JobStatus, TaskRecord } from "@/types";

export function TaskStatus({ status, compact = false }: { status: JobStatus; compact?: boolean }) {
  const meta = statusMeta[status];
  const active = activeStatuses.includes(status);
  const Icon = meta.tone === "success" ? Check : meta.tone === "danger" ? CircleAlert : active ? LoaderCircle : Circle;

  return (
    <span
      className={cn(
        "inline-flex min-w-0 items-center gap-1.5 text-xs",
        meta.tone === "success" && "text-success",
        meta.tone === "danger" && "text-destructive",
        meta.tone === "active" && "text-foreground",
        meta.tone === "neutral" && "text-muted-foreground",
      )}
    >
      <Icon className={cn("size-3.5 shrink-0", active && "animate-spin")} />
      {!compact && <span className="truncate">{meta.label}</span>}
    </span>
  );
}

export function TaskProgress({ task }: { task: TaskRecord }) {
  const hasParts = task.kind === "document"
    && typeof task.partCompletedCount === "number"
    && typeof task.partCount === "number"
    && task.partCount > 0;
  const extracted = task.mineruExtractedPages;
  const total = task.mineruTotalPages;
  const hasPages = task.mineruState === "running" && typeof extracted === "number" && typeof total === "number" && total > 0;
  const percentage = hasPages ? Math.min(100, Math.max(0, Math.round((extracted / total) * 100))) : undefined;
  const labels: Record<string, string> = {
    "waiting-file": "等待 MinerU 接收文件",
    pending: "MinerU 排队中",
    running: "MinerU 正在逐页解析",
    converting: "MinerU 正在生成结果",
    done: "MinerU 解析完成",
    failed: "MinerU 解析失败",
  };
  const failedParts = task.partFailedCount ?? 0;
  const label = hasParts
    ? failedParts > 0
      ? `${failedParts} 个分片失败，等待重试`
      : "MinerU 正在处理 PDF 分片"
    : task.engine === "mineru" ? labels[task.mineruState ?? ""] ?? statusMeta[task.status].label : statusMeta[task.status].label;

  return (
    <div>
      <div className="mb-1.5 flex items-center justify-between gap-3 text-[11px] text-muted-foreground">
        <span className="truncate">{label}</span>
        {hasParts ? (
          <span className="shrink-0 tabular-nums">{task.partCompletedCount}/{task.partCount} 片</span>
        ) : hasPages && <span className="shrink-0 tabular-nums">{extracted}/{total} 页 · {percentage}%</span>}
      </div>
      <div className="h-1 overflow-hidden rounded-full bg-muted" role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={hasParts ? task.partCount : hasPages ? total : undefined} aria-valuenow={hasParts ? task.partCompletedCount : hasPages ? extracted : undefined}>
        {hasParts || hasPages ? (
          <div className="h-full rounded-full bg-primary transition-[width] duration-300" style={{ width: `${hasParts ? Math.round(((task.partCompletedCount ?? 0) / (task.partCount ?? 1)) * 100) : percentage}%` }} />
        ) : (
          <div className="progress-indeterminate h-full w-1/3 rounded-full bg-primary/80" />
        )}
      </div>
    </div>
  );
}
