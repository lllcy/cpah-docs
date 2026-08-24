import { useState } from "react";
import {
  ArrowRight,
  BookOpenText,
  Bot,
  CheckCircle2,
  CloudUpload,
  Copy,
  FolderInput,
  FolderOutput,
  ListTodo,
  LoaderCircle,
  Scale,
  Settings2,
  ShieldCheck,
  Tags,
  TriangleAlert,
  Stethoscope,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { HealthReport } from "@/types";

type HelpViewProps = {
  hasProfiles: boolean;
  onStartSetup: () => void;
  onOpenFormats: () => void;
  onOpenSettings: () => void;
  onOpenConversionTasks: () => void;
  onOpenClassificationTasks: () => void;
  healthReport: HealthReport | null;
  appVersion: string;
  checkingHealth: boolean;
  copyingDiagnostics: boolean;
  onRunHealthCheck: () => void;
  onCopyDiagnostics: () => void;
  onLoadProjectLicense: () => Promise<string>;
  onLoadThirdPartyLicenses: () => Promise<string>;
};

const quickSteps = [
  ["创建监控目录", "建立一个专门存放 Word、PDF、Excel、PPT 等原始资料的文件夹。"],
  ["创建输出目录", "再建立一个独立文件夹，用来接收 Markdown、附件资源和知识库索引。"],
  ["添加目录并保存", "分别选择监控目录和输出目录，确认目录配置已保存。"],
  ["确认并开始转换", "监听会先把文件加入待执行；确认数量后，在转换任务页点击“开始转换”。"],
] as const;

const faqItems = [
  ["为什么需要两个文件夹？", "原始文档和转换结果分开后，程序可以安全镜像目录结构，也不会重复处理刚生成的 Markdown。"],
  ["两个文件夹可以互相包含吗？", "不可以。它们不能相同或互相包含，建议在同一上级目录下并列创建。"],
  ["文件没有转换成功怎么办？", "先在转换任务页查看错误。大型 PDF 可单独重试失败分片；常见原因还包括文件被占用、文档加密、格式开关关闭，或 MinerU 尚未配置。"],
  ["关闭窗口后程序还会运行吗？", "会。关闭主窗口后程序会留在 Windows 系统托盘或 macOS 菜单栏；需要完全关闭时，请在图标菜单中选择“退出”。"],
] as const;

const isMacOS = navigator.userAgent.includes("Mac");
const sourcePathExample = isMacOS
  ? "/Users/you/Documents/原始资料/项目A/报告.pdf"
  : "D:\\我的文档\\原始资料\\项目A\\报告.pdf";
const outputPathExample = isMacOS
  ? "/Users/you/Documents/Markdown输出/项目A/报告.md"
  : "D:\\我的文档\\Markdown输出\\项目A\\报告.md";
const assetPathExample = isMacOS
  ? "/Users/you/Documents/Markdown输出/项目A/报告.assets/"
  : "D:\\我的文档\\Markdown输出\\项目A\\报告.assets\\";

function formatGeneratedLicenseDocument(source: string) {
  const text = source
    .replace(/<details>\s*/g, "")
    .replace(/<\/details>\s*/g, "\n")
    .replace(/<summary>(.*?)<\/summary>/g, "$1\n")
    .replace(/<\/?pre>/g, "")
    .replace(/\[([^\]]+)]\(([^)]+)\)/g, "$1 — $2")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/^>\s?/gm, "");
  const decoder = document.createElement("textarea");
  decoder.innerHTML = text;
  return decoder.value.replace(/\n{3,}/g, "\n\n").trim();
}

export function HelpView({ hasProfiles, onStartSetup, onOpenFormats, onOpenSettings, onOpenConversionTasks, onOpenClassificationTasks, healthReport, appVersion, checkingHealth, copyingDiagnostics, onRunHealthCheck, onCopyDiagnostics, onLoadProjectLicense, onLoadThirdPartyLicenses }: HelpViewProps) {
  const [projectLicense, setProjectLicense] = useState<string | null>(null);
  const [thirdPartyLicenses, setThirdPartyLicenses] = useState<string | null>(null);
  const [loadingProjectLicense, setLoadingProjectLicense] = useState(false);
  const [loadingThirdPartyLicenses, setLoadingThirdPartyLicenses] = useState(false);

  async function loadProjectLicense() {
    if (projectLicense !== null || loadingProjectLicense) return;
    setLoadingProjectLicense(true);
    try {
      setProjectLicense(await onLoadProjectLicense());
    } catch (error) {
      setProjectLicense(`无法读取 CPAH Docs 许可证：${String(error)}`);
    } finally {
      setLoadingProjectLicense(false);
    }
  }

  async function loadThirdPartyLicenses() {
    if (thirdPartyLicenses !== null || loadingThirdPartyLicenses) return;
    setLoadingThirdPartyLicenses(true);
    try {
      setThirdPartyLicenses(formatGeneratedLicenseDocument(await onLoadThirdPartyLicenses()));
    } catch (error) {
      setThirdPartyLicenses(`无法读取第三方许可证：${String(error)}`);
    } finally {
      setLoadingThirdPartyLicenses(false);
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div className="flex min-h-[62px] shrink-0 items-center border-b px-5 max-[900px]:px-4">
        <div>
          <h1 className="text-[15px] font-semibold tracking-[-0.01em]">新手引导与帮助</h1>
          <p className="mt-0.5 text-[11px] text-muted-foreground">准备监控目录和输出目录，开始自动转换</p>
        </div>
        <div className="ml-auto flex items-center gap-3"><span className="text-[10px] text-muted-foreground">v{appVersion}</span><Button size="sm" onClick={onStartSetup}><FolderInput />{hasProfiles ? "管理监控目录" : "开始配置"}</Button></div>
      </div>

      <section className="min-h-0 flex-1 overflow-y-auto">
        <div className="border-b">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-5 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <div className="flex items-center gap-2"><FolderInput className="size-3.5 text-muted-foreground" /><h2 className="text-xs font-medium">快速开始</h2></div>
              <p className="mt-2 text-[10px] leading-4 text-muted-foreground">先准备两个独立文件夹</p>
            </div>
            <div>
              <div className="divide-y border-y">
                {quickSteps.map(([title, description], index) => (
                  <div key={title} className="grid min-h-[52px] grid-cols-[28px_130px_minmax(0,1fr)] items-center gap-3 py-2.5 max-[720px]:grid-cols-[28px_minmax(0,1fr)]">
                    <span className="font-mono text-[10px] tabular-nums text-muted-foreground">{String(index + 1).padStart(2, "0")}</span>
                    <p className="text-[11px] font-medium max-[720px]:col-start-2">{title}</p>
                    <p className="text-[10px] leading-4 text-muted-foreground max-[720px]:col-start-2">{description}</p>
                  </div>
                ))}
              </div>
              <div className="mt-3 flex items-start gap-2 text-[10px] leading-4 text-amber-700 dark:text-amber-300">
                <TriangleAlert className="mt-px size-3.5 shrink-0" />
                <p>监控目录和输出目录不能相同或互相包含，推荐在同一上级目录下并列创建。</p>
              </div>
              <div className="mt-4 flex flex-wrap gap-2">
                <Button size="sm" onClick={onStartSetup}><FolderInput />{hasProfiles ? "管理监控目录" : "开始配置目录"}</Button>
                <Button variant="outline" size="sm" onClick={onOpenFormats}><BookOpenText />支持格式</Button>
                <Button variant="outline" size="sm" onClick={onOpenConversionTasks}><ListTodo />转换任务</Button>
              </div>
            </div>
          </div>
        </div>

        <div className="border-b bg-[var(--table-head)]">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-5 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <div className="flex items-center gap-2"><FolderOutput className="size-3.5 text-muted-foreground" /><h2 className="text-xs font-medium">目录关系</h2></div>
              <p className="mt-2 text-[10px] leading-4 text-muted-foreground">保持子文件夹结构</p>
            </div>
            <div>
              <div className="grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-3 max-[720px]:grid-cols-1">
                <div className="min-w-0">
                  <p className="text-[10px] font-medium text-muted-foreground">监控目录 · 原始文档</p>
                  <code className="mt-1 block break-all text-[11px] leading-5">{sourcePathExample}</code>
                </div>
                <ArrowRight className="size-3.5 text-muted-foreground max-[720px]:rotate-90" />
                <div className="min-w-0">
                  <p className="text-[10px] font-medium text-muted-foreground">输出目录 · Markdown</p>
                  <code className="mt-1 block break-all text-[11px] leading-5">{outputPathExample}</code>
                  <code className="block break-all text-[10px] leading-4 text-muted-foreground">{assetPathExample}</code>
                </div>
              </div>
              <div className="mt-4 border-t pt-3 text-[10px] leading-4 text-muted-foreground">
                输出根目录和包含文档的子目录都会自动维护 <code className="text-foreground">index.md</code>，同时按文件夹和标签提供导航；生成索引不调用模型，也不消耗 Token。
              </div>
            </div>
          </div>
        </div>

        <div className="border-b">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-5 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <h2 className="text-xs font-medium">运行与服务</h2>
              <p className="mt-2 text-[10px] leading-4 text-muted-foreground">转换、分类与数据边界</p>
            </div>
            <div className="divide-y border-y">
              <div className="grid min-h-[66px] grid-cols-[20px_minmax(0,1fr)_auto] items-center gap-3 py-3">
                <CloudUpload className="size-3.5 text-muted-foreground" />
                <div><p className="text-[11px] font-medium">本地转换与 MinerU</p><p className="mt-1 text-[10px] leading-4 text-muted-foreground">Office、文本优先在本地处理；PDF、图片和旧版 Office 使用 MinerU。</p></div>
                <Button variant="ghost" size="sm" onClick={onOpenSettings}><Settings2 />设置</Button>
              </div>
              <div className="grid min-h-[66px] grid-cols-[20px_minmax(0,1fr)_auto] items-center gap-3 py-3">
                <Bot className="size-3.5 text-muted-foreground" />
                <div><p className="text-[11px] font-medium">Agent 文档分类（可选）</p><p className="mt-1 text-[10px] leading-4 text-muted-foreground">从候选类别中选择分类并写入 Markdown YAML。</p></div>
                <Button variant="ghost" size="sm" onClick={onOpenClassificationTasks}><Tags />分类任务</Button>
              </div>
              <div className="grid min-h-[66px] grid-cols-[20px_minmax(0,1fr)_auto] items-center gap-3 py-3">
                <ListTodo className="size-3.5 text-muted-foreground" />
                <div><p className="text-[11px] font-medium">监听、转换与分类</p><p className="mt-1 text-[10px] leading-4 text-muted-foreground">监听只负责发现文件并加入待执行；转换和分类分别在各自任务页独立控制。新用户默认先监听、后手动开始转换。</p></div>
                <Button variant="ghost" size="sm" onClick={onOpenConversionTasks}>转换任务</Button>
              </div>
              <div className="grid min-h-[66px] grid-cols-[20px_minmax(0,1fr)] items-center gap-3 py-3">
                <ShieldCheck className="size-3.5 text-muted-foreground" />
                <div><p className="text-[11px] font-medium">数据与凭据</p><p className="mt-1 text-[10px] leading-4 text-muted-foreground">任务记录保存在本机；Token 和 API Key 保存在系统凭据库（Windows 凭据管理器或 macOS 钥匙串）。</p></div>
              </div>
            </div>
          </div>
        </div>

        <div className="border-b bg-[var(--table-head)]">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-5 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <div className="flex items-center gap-2"><Stethoscope className="size-3.5 text-muted-foreground" /><h2 className="text-xs font-medium">运行诊断</h2></div>
              <p className="mt-2 text-[10px] leading-4 text-muted-foreground">只检查本机状态，不上传文档</p>
            </div>
            <div>
              <div className="flex flex-wrap items-center gap-2">
                <Button size="sm" variant="outline" disabled={checkingHealth} onClick={onRunHealthCheck}>{checkingHealth ? <LoaderCircle className="animate-spin" /> : <Stethoscope />}运行检查</Button>
                <Button size="sm" variant="outline" disabled={copyingDiagnostics} onClick={onCopyDiagnostics}>{copyingDiagnostics ? <LoaderCircle className="animate-spin" /> : <Copy />}复制诊断信息</Button>
                {healthReport && <span className={cn("text-[10px] font-medium", healthReport.overall === "ok" ? "text-success" : healthReport.overall === "warning" ? "text-amber-600 dark:text-amber-400" : "text-destructive")}>{healthReport.overall === "ok" ? "全部正常" : healthReport.overall === "warning" ? "存在提醒" : "发现异常"}</span>}
              </div>
              {healthReport ? (
                <div className="mt-3 divide-y border-y">
                  {healthReport.checks.map((check) => (
                    <div key={check.id} className="grid grid-cols-[18px_120px_minmax(0,1fr)] gap-3 py-2.5 text-[10px] leading-4 max-[720px]:grid-cols-[18px_minmax(0,1fr)]">
                      {check.level === "ok" ? <CheckCircle2 className="mt-px size-3.5 text-success" /> : <TriangleAlert className={cn("mt-px size-3.5", check.level === "error" ? "text-destructive" : "text-amber-600 dark:text-amber-400")} />}
                      <p className="font-medium max-[720px]:col-start-2">{check.title}</p>
                      <div className="text-muted-foreground max-[720px]:col-start-2"><p>{check.detail}</p>{check.suggestion && <p className="mt-1 text-foreground/75">建议：{check.suggestion}</p>}</div>
                    </div>
                  ))}
                </div>
              ) : <p className="mt-3 text-[10px] leading-4 text-muted-foreground">可检查目录权限、设置备份、数据库、MinerU/Agent 配置以及三个后台服务。复制报告时会隐藏凭据和完整路径。</p>}
            </div>
          </div>
        </div>

        <div className="border-b">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-5 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <h2 className="text-xs font-medium">常见问题</h2>
              <p className="mt-2 text-[10px] leading-4 text-muted-foreground">配置与运行说明</p>
            </div>
            <div className="divide-y border-y">
              {faqItems.map(([question, answer]) => (
                <details key={question} className="group">
                  <summary className="flex min-h-[44px] cursor-pointer list-none items-center gap-3 py-2.5 text-[11px] font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring">
                    <span>{question}</span>
                    <span className="ml-auto text-muted-foreground transition-transform group-open:rotate-45">＋</span>
                  </summary>
                  <p className="pb-3 pr-7 text-[10px] leading-5 text-muted-foreground">{answer}</p>
                </details>
              ))}
            </div>
          </div>
        </div>

        <div className="border-b bg-[var(--table-head)]">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-5 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <div className="flex items-center gap-2"><Scale className="size-3.5 text-muted-foreground" /><h2 className="text-xs font-medium">开源许可</h2></div>
              <p className="mt-2 text-[10px] leading-4 text-muted-foreground">原创代码以 MIT License 授权；第三方组件适用各自许可证</p>
            </div>
            <div className="divide-y border-y">
              <details className="group" onToggle={(event) => { if (event.currentTarget.open) void loadProjectLicense(); }}>
                <summary className="flex min-h-[44px] cursor-pointer list-none items-center gap-3 py-2.5 text-[11px] font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring">
                  <span>查看 CPAH Docs MIT 许可证</span>
                  <span className="ml-auto text-muted-foreground transition-transform group-open:rotate-45">＋</span>
                </summary>
                <pre className="mb-3 max-h-72 overflow-auto whitespace-pre-wrap break-words border bg-background p-3 text-[10px] leading-4 text-muted-foreground">{loadingProjectLicense ? "正在读取…" : projectLicense}</pre>
              </details>
              <details className="group" onToggle={(event) => { if (event.currentTarget.open) void loadThirdPartyLicenses(); }}>
                <summary className="flex min-h-[44px] cursor-pointer list-none items-center gap-3 py-2.5 text-[11px] font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring">
                  <span>查看第三方软件许可与声明</span>
                  <span className="ml-auto text-muted-foreground transition-transform group-open:rotate-45">＋</span>
                </summary>
                <pre className="mb-3 max-h-72 overflow-auto whitespace-pre-wrap break-words border bg-background p-3 text-[10px] leading-4 text-muted-foreground">{loadingThirdPartyLicenses ? "正在读取…" : thirdPartyLicenses}</pre>
              </details>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
