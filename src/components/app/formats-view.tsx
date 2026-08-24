import {
  ArrowRight,
  Check,
  CloudUpload,
  FileArchive,
  FileImage,
  FileSpreadsheet,
  FileText,
  FileType2,
  HardDrive,
  KeyRound,
  Presentation,
  Settings2,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import type { DirectorySaveState } from "@/app-model";
import { cn } from "@/lib/utils";

type FormatRow = {
  name: string;
  extensions: string[];
  icon: typeof FileText;
  note: string;
};

const localFormats: FormatRow[] = [
  { name: "Markdown", extensions: ["md"], icon: FileText, note: "原样同步，不改写内容或添加元数据" },
  { name: "Word 文档", extensions: ["docx"], icon: FileText, note: "提取正文、表格和内嵌图片" },
  { name: "Excel 工作簿", extensions: ["xlsx", "xls"], icon: FileSpreadsheet, note: "按工作表转换为 Markdown" },
  { name: "PowerPoint 演示文稿", extensions: ["pptx"], icon: Presentation, note: "按幻灯片顺序提取内容" },
  { name: "网页文档", extensions: ["html", "htm"], icon: FileType2, note: "保留正文结构和链接" },
  { name: "表格数据", extensions: ["csv"], icon: FileSpreadsheet, note: "转换为 Markdown 表格" },
  { name: "纯文本", extensions: ["txt"], icon: FileText, note: "直接生成 Markdown 文本" },
];

const cloudFormats: FormatRow[] = [
  { name: "PDF 文档", extensions: ["pdf"], icon: FileArchive, note: "自动处理 200 页或 200 MB 以上的大文件" },
  { name: "旧版 Word", extensions: ["doc"], icon: FileText, note: "上传 MinerU 解析" },
  { name: "旧版 PowerPoint", extensions: ["ppt"], icon: Presentation, note: "上传 MinerU 解析" },
  { name: "图片与扫描件", extensions: ["png", "jpg", "jpeg", "webp", "bmp"], icon: FileImage, note: "通过 OCR 识别文字与版面" },
];

function FormatList({
  rows,
  enabledExtensions,
  onToggleExtensions,
}: {
  rows: FormatRow[];
  enabledExtensions: string[];
  onToggleExtensions: (extensions: string[], enabled: boolean) => void;
}) {
  return (
    <div>
      {rows.map(({ name, extensions, icon: Icon, note }) => {
        const enabled = extensions.every((extension) => enabledExtensions.includes(extension));
        return (
        <div key={name} className={cn("grid min-h-[58px] grid-cols-[minmax(160px,.8fr)_minmax(150px,1fr)_auto] items-center gap-4 border-b px-5 py-2.5 last:border-b-0 max-[900px]:grid-cols-[minmax(0,1fr)_auto] max-[900px]:gap-1.5 max-[900px]:px-4", !enabled && "bg-muted/25")}>
          <div className="flex min-w-0 items-center gap-2.5">
            <Icon className={cn("size-3.5 shrink-0", enabled ? "text-foreground" : "text-muted-foreground/55")} />
            <div className="min-w-0">
              <p className={cn("truncate text-xs font-medium", !enabled && "text-muted-foreground")}>{name}</p>
              <p className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">{extensions.map((extension) => `.${extension}`).join("  ")}</p>
            </div>
          </div>
          <p className="text-[10px] leading-4 text-muted-foreground max-[900px]:col-start-1">{enabled ? note : "已停用：不会扫描或加入转换队列"}</p>
          <Switch checked={enabled} onCheckedChange={(checked) => onToggleExtensions(extensions, checked)} aria-label={`${enabled ? "停用" : "启用"}${name}`} />
        </div>
      );})}
    </div>
  );
}

type FormatsViewProps = {
  mineruConfigured: boolean;
  enabledExtensions: string[];
  saveState: DirectorySaveState;
  onToggleExtensions: (extensions: string[], enabled: boolean) => void;
  onOpenSettings: () => void;
};

export function FormatsView({ mineruConfigured, enabledExtensions, saveState, onToggleExtensions, onOpenSettings }: FormatsViewProps) {
  const localExtensions = localFormats.flatMap((format) => format.extensions);
  const cloudExtensions = cloudFormats.flatMap((format) => format.extensions);
  const enabledLocal = localExtensions.filter((extension) => enabledExtensions.includes(extension)).length;
  const enabledCloud = cloudExtensions.filter((extension) => enabledExtensions.includes(extension)).length;
  const saveLabel = saveState === "saving" ? "正在保存" : saveState === "saved" ? "开关已保存" : saveState === "error" ? "保存失败" : "等待自动保存";
  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div className="flex min-h-[62px] shrink-0 items-center border-b px-5 max-[900px]:px-4">
        <div>
          <h1 className="text-[15px] font-semibold tracking-[-0.01em]">格式设置</h1>
          <p className="mt-0.5 text-[11px] text-muted-foreground">选择需要监控和转换的文档格式</p>
        </div>
        <span className={cn("ml-auto text-[10px]", saveState === "error" ? "text-destructive" : "text-muted-foreground")}>{saveLabel}</span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <section className="grid grid-cols-3 divide-x border-b bg-[var(--table-head)]">
          <div className="flex h-[52px] items-center gap-2.5 px-5 max-[900px]:px-3">
            <FileType2 className="size-3.5 text-muted-foreground" />
            <div><p className="text-[10px] text-muted-foreground">已启用扩展名</p><p className="mt-0.5 text-sm font-semibold tabular-nums">{enabledExtensions.length} / 17</p></div>
          </div>
          <div className="flex h-[52px] items-center gap-2.5 px-5 max-[900px]:px-3">
            <HardDrive className="size-3.5 text-foreground" />
            <div><p className="text-[10px] text-muted-foreground">本地处理</p><p className="mt-0.5 text-sm font-semibold tabular-nums">{enabledLocal} / {localExtensions.length}</p></div>
          </div>
          <div className="flex h-[52px] items-center gap-2.5 px-5 max-[900px]:px-3">
            <CloudUpload className="size-3.5 text-foreground" />
            <div><p className="text-[10px] text-muted-foreground">云端处理</p><p className="mt-0.5 text-sm font-semibold tabular-nums">{enabledCloud} / {cloudExtensions.length}</p></div>
          </div>
        </section>

        <div className="grid grid-cols-2 max-[760px]:grid-cols-1">
          <section className="min-w-0 border-r max-[760px]:border-b max-[760px]:border-r-0">
            <div className="flex min-h-[66px] items-center gap-3 border-b px-5 max-[900px]:px-4">
              <HardDrive className="size-4 text-foreground" />
              <div className="min-w-0">
                <h2 className="text-xs font-semibold">本地转换</h2>
                <p className="mt-1 text-[10px] leading-4 text-muted-foreground">Office 与文本由 AnyToMD 处理，Markdown 原样同步；均不会上传。</p>
              </div>
            </div>
            <FormatList rows={localFormats} enabledExtensions={enabledExtensions} onToggleExtensions={onToggleExtensions} />
          </section>

          <section className="min-w-0">
            <div className="flex min-h-[66px] items-center gap-3 border-b px-5 max-[900px]:px-4">
              <CloudUpload className="size-4 text-foreground" />
              <div className="min-w-0 flex-1">
                <h2 className="text-xs font-semibold">MinerU 云端转换</h2>
                <p className="mt-1 text-[10px] leading-4 text-muted-foreground">源文件会上传到 MinerU，需要网络和有效 Token；大型 PDF 会先在本地无损分片。</p>
              </div>
              <span className={cn("inline-flex shrink-0 items-center gap-1.5 text-[10px]", mineruConfigured ? "text-success" : "text-muted-foreground")}>
                {mineruConfigured ? <Check className="size-3" /> : <KeyRound className="size-3" />}
                {mineruConfigured ? "Token 已配置" : "未配置 Token"}
              </span>
            </div>
            <FormatList rows={cloudFormats} enabledExtensions={enabledExtensions} onToggleExtensions={onToggleExtensions} />
            {!mineruConfigured && (
              <div className="flex items-center gap-3 border-t px-5 py-3 max-[900px]:px-4">
                <p className="min-w-0 flex-1 text-[10px] leading-4 text-muted-foreground">未配置时，云端文件会进入“等待 MinerU”，配置后可继续重试。</p>
                <Button variant="outline" size="sm" onClick={onOpenSettings}><Settings2 />前往设置</Button>
              </div>
            )}
          </section>
        </div>

        <section className="border-t bg-[var(--table-head)]">
          <div className="grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-3 px-5 py-4 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <p className="text-[10px] font-medium text-foreground">监控与路由</p>
              <p className="mt-1 text-[10px] leading-4 text-muted-foreground">只扫描以上扩展名，其他文件会被忽略。</p>
            </div>
            <ArrowRight className="size-3.5 text-muted-foreground max-[760px]:rotate-90" />
            <div>
              <p className="text-[10px] font-medium text-foreground">保持目录结构</p>
              <p className="mt-1 text-[10px] leading-4 text-muted-foreground">输入目录下的子文件夹会同步到输出目录。</p>
            </div>
            <ArrowRight className="size-3.5 text-muted-foreground max-[760px]:rotate-90" />
            <div>
              <p className="text-[10px] font-medium text-foreground">生成 Markdown</p>
              <p className="mt-1 text-[10px] leading-4 text-muted-foreground">默认输出“原文件名.md”；同名冲突时保留原扩展名。</p>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
