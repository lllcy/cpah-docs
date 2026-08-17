import { useEffect, useState } from "react";
import { Bot, ExternalLink, KeyRound, Laptop, LoaderCircle, Moon, PlugZap, Scissors, ShieldCheck, Sun } from "lucide-react";

import type { ThemeMode } from "@/app-model";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import type { AgentSettings } from "@/types";

type SplitSettings = {
  splitEnabled: boolean;
  splitMaxPages: number;
  splitOverlapPages: number;
  splitTempDir: string | null;
  splitKeepTemp: boolean;
};

type SettingsViewProps = {
  appVersion: string;
  theme: ThemeMode;
  onThemeChange: (theme: ThemeMode) => void;
  mineruConfigured: boolean;
  mineruBaseUrl: string;
  token: string;
  onTokenChange: (token: string) => void;
  savingToken: boolean;
  onSaveToken: () => void;
  onOpenMineruTokenPage: () => void;
  agent: AgentSettings;
  onSaveAgent: (value: { baseUrl: string; model: string; apiKey: string; concurrency: number }) => Promise<void>;
  onTestAgent: (value: { baseUrl: string; model: string; apiKey: string }) => Promise<void>;
  splitSettings: SplitSettings;
  onSaveSplit: (next: SplitSettings) => void;
};

export function SettingsView({ appVersion, theme, onThemeChange, mineruConfigured, mineruBaseUrl, token, onTokenChange, savingToken, onSaveToken, onOpenMineruTokenPage, agent, onSaveAgent, onTestAgent, splitSettings, onSaveSplit }: SettingsViewProps) {
  const [baseUrl, setBaseUrl] = useState(agent.baseUrl);
  const [model, setModel] = useState(agent.model);
  const [apiKey, setApiKey] = useState("");
  const [concurrency, setConcurrency] = useState(agent.concurrency);
  const [savingAgent, setSavingAgent] = useState(false);
  const [testingAgent, setTestingAgent] = useState(false);
  useEffect(() => {
    setBaseUrl(agent.baseUrl);
    setModel(agent.model);
    setConcurrency(agent.concurrency);
  }, [agent.baseUrl, agent.concurrency, agent.model]);

  async function saveAgent() {
    setSavingAgent(true);
    try {
      await onSaveAgent({ baseUrl, model, apiKey, concurrency });
      setApiKey("");
    } finally {
      setSavingAgent(false);
    }
  }

  async function testAgent() {
    setTestingAgent(true);
    try {
      await onTestAgent({ baseUrl, model, apiKey });
    } finally {
      setTestingAgent(false);
    }
  }

  const [splitEnabled, setSplitEnabled] = useState(splitSettings.splitEnabled);
  const [splitMaxPages, setSplitMaxPages] = useState(splitSettings.splitMaxPages);
  const [splitOverlapPages, setSplitOverlapPages] = useState(splitSettings.splitOverlapPages);
  const [splitTempDir, setSplitTempDir] = useState(splitSettings.splitTempDir ?? "");
  const [splitKeepTemp, setSplitKeepTemp] = useState(splitSettings.splitKeepTemp);
  const [savingSplit, setSavingSplit] = useState(false);
  useEffect(() => {
    setSplitEnabled(splitSettings.splitEnabled);
    setSplitMaxPages(splitSettings.splitMaxPages);
    setSplitOverlapPages(splitSettings.splitOverlapPages);
    setSplitTempDir(splitSettings.splitTempDir ?? "");
    setSplitKeepTemp(splitSettings.splitKeepTemp);
  }, [splitSettings.splitEnabled, splitSettings.splitMaxPages, splitSettings.splitOverlapPages, splitSettings.splitTempDir, splitSettings.splitKeepTemp]);

  async function saveSplit() {
    setSavingSplit(true);
    try {
      const maxPages = Math.max(1, Math.floor(splitMaxPages) || 200);
      const overlap = Math.max(0, Math.min(Math.floor(splitOverlapPages) || 0, maxPages - 1));
      const tempDir = splitTempDir.trim() === "" ? null : splitTempDir.trim();
      onSaveSplit({ splitEnabled, splitMaxPages: maxPages, splitOverlapPages: overlap, splitTempDir: tempDir, splitKeepTemp });
    } finally {
      setSavingSplit(false);
    }
  }
  const themes = [
    { id: "system" as const, label: "跟随系统", icon: Laptop },
    { id: "light" as const, label: "浅色", icon: Sun },
    { id: "dark" as const, label: "深色", icon: Moon },
  ];

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div className="flex min-h-[62px] shrink-0 items-center border-b px-5 max-[900px]:px-4">
        <div>
          <h1 className="text-[15px] font-semibold tracking-[-0.01em]">设置</h1>
          <p className="mt-0.5 text-[11px] text-muted-foreground">监控行为、外观与文档解析服务</p>
        </div>
      </div>

      <section className="min-h-0 flex-1 overflow-y-auto">
        <div className="border-b">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-4 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <div className="flex items-center gap-2">
                <KeyRound className="size-3.5 text-muted-foreground" />
                <h2 className="text-xs font-medium">MinerU Token</h2>
              </div>
              <div className={cn("mt-2 inline-flex items-center gap-1.5 text-[10px]", mineruConfigured ? "text-success" : "text-muted-foreground")}>
                <ShieldCheck className="size-3" />{mineruConfigured ? "已配置" : "尚未配置"}
              </div>
              <div className="mt-2">
                <Button variant="outline" size="sm" onClick={onOpenMineruTokenPage}><ExternalLink />申请 Token</Button>
              </div>
            </div>
            <div>
              <p className="mb-2 text-[10px] leading-4 text-muted-foreground">PDF 优先使用 MinerU 解析。Token 只交给本机 Tauri 后端保存。</p>
              <div className="flex gap-2">
                <Input type="password" value={token} onChange={(event) => onTokenChange(event.target.value)} placeholder={mineruConfigured ? "输入新 Token 以替换现有配置" : "输入 MinerU Token"} />
                <Button disabled={!token.trim() || savingToken} onClick={onSaveToken}>{savingToken ? <LoaderCircle className="animate-spin" /> : <ShieldCheck />}{mineruConfigured ? "更新" : "保存"}</Button>
              </div>
              <p className="mt-2 truncate text-[10px] text-muted-foreground">API 地址：{mineruBaseUrl}</p>
            </div>
          </div>
        </div>

        <div className="border-b">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-4 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <div className="flex items-center gap-2"><Scissors className="size-3.5 text-muted-foreground" /><h2 className="text-xs font-medium">PDF 自动拆分</h2></div>
              <p className="mt-2 text-[10px] text-muted-foreground">仅对 PDF 生效</p>
            </div>
            <div className="min-w-0 space-y-3">
              <div className="flex items-center justify-between gap-3 rounded-md border bg-card px-3 py-2.5">
                <div className="min-w-0">
                  <p className="text-[11px] font-medium leading-5">超过页数时自动拆分</p>
                  <p className="mt-0.5 text-[10px] leading-4 text-muted-foreground">MinerU 单次解析限制约 200 页；超限时按批次拆分后逐块解析并合并结果。</p>
                </div>
                <Switch checked={splitEnabled} onCheckedChange={setSplitEnabled} aria-label="启用 PDF 自动拆分" />
              </div>
              <div className="grid grid-cols-2 gap-2">
                <label className="space-y-1"><span className="text-[10px] text-muted-foreground">每批最大页数</span><Input type="number" min={1} value={splitMaxPages} onChange={(event) => setSplitMaxPages(Number(event.target.value))} /></label>
                <label className="space-y-1"><span className="text-[10px] text-muted-foreground">批次重叠页数</span><Input type="number" min={0} value={splitOverlapPages} onChange={(event) => setSplitOverlapPages(Number(event.target.value))} /></label>
              </div>
              <label className="block space-y-1">
                <span className="text-[10px] text-muted-foreground">拆分临时目录（留空则使用输出目录下的 .cpah-split）</span>
                <Input value={splitTempDir} onChange={(event) => setSplitTempDir(event.target.value)} placeholder="留空使用默认临时目录" />
              </label>
              <div className="flex items-center justify-between gap-3 rounded-md border bg-card px-3 py-2.5">
                <div className="min-w-0">
                  <p className="text-[11px] font-medium leading-5">保留拆分临时文件</p>
                  <p className="mt-0.5 text-[10px] leading-4 text-muted-foreground">默认在合并完成后清理临时 PDF；开启后保留以便排查。</p>
                </div>
                <Switch checked={splitKeepTemp} onCheckedChange={setSplitKeepTemp} aria-label="保留拆分临时文件" />
              </div>
              <p className="text-[10px] leading-4 text-muted-foreground">拆分与合并过程会写入日志；输出 Markdown 中以 <code className="break-all text-foreground">&lt;!-- cpah_split --&gt;</code> 标记各块对应的原始页码区间，并生成 <code className="break-all text-foreground">.md.pagemap.json</code> 页码映射文件。</p>
              <div className="flex justify-end">
                <Button disabled={savingSplit} onClick={() => void saveSplit()}>{savingSplit ? <LoaderCircle className="animate-spin" /> : <ShieldCheck />}保存拆分设置</Button>
              </div>
            </div>
          </div>
        </div>

        <div className="border-b">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-4 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <div className="flex items-center gap-2"><Bot className="size-3.5 text-muted-foreground" /><h2 className="text-xs font-medium">Agent 模型</h2></div>
              <div className={cn("mt-2 inline-flex items-center gap-1.5 text-[10px]", agent.configured ? "text-success" : "text-muted-foreground")}><ShieldCheck className="size-3" />{agent.configured ? "API Key 已配置" : "尚未配置"}</div>
            </div>
            <div className="min-w-0 space-y-3">
              <div className="min-w-0 border-l-2 border-primary/35 pl-3">
                <p className="text-[11px] font-medium leading-5">用于 Agent 文档分类</p>
                <p className="mt-0.5 break-words text-[10px] leading-4 text-muted-foreground">读取输出目录中的 Markdown，从监控目录配置的候选类别中选择，并把结果写入 YAML 的 <code className="break-all text-foreground">cpah_categories</code>。</p>
                <p className="mt-1 break-words text-[10px] leading-4 text-muted-foreground">只有目录已开启分类且“分类任务”正在运行时才会调用模型；不参与文档转换、目录同步、MinerU 解析或索引生成。</p>
              </div>
              <p className="break-words text-[10px] leading-4 text-muted-foreground">支持 OpenAI 兼容的 Chat Completions Tool Calling。API Key 只保存到系统凭据库。</p>
              <div className="grid grid-cols-[minmax(0,1fr)_180px] gap-2 max-[720px]:grid-cols-1">
                <label className="space-y-1"><span className="text-[10px] text-muted-foreground">Base URL</span><Input value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} placeholder="https://api.openai.com/v1" /></label>
                <label className="space-y-1"><span className="text-[10px] text-muted-foreground">模型名称</span><Input value={model} onChange={(event) => setModel(event.target.value)} placeholder="gpt-4.1-mini" /></label>
              </div>
              <div className="grid grid-cols-[minmax(0,1fr)_120px] gap-2">
                <label className="space-y-1"><span className="text-[10px] text-muted-foreground">API Key</span><Input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={agent.configured ? "留空则继续使用已保存的 Key" : "输入 API Key"} /></label>
                <label className="space-y-1"><span className="text-[10px] text-muted-foreground">并发数</span><select value={concurrency} onChange={(event) => setConcurrency(Number(event.target.value))} className="h-8 w-full rounded-md border border-input bg-card px-2.5 text-xs outline-none focus:border-ring focus:ring-2 focus:ring-ring/20">{[1, 2, 3, 4].map((value) => <option key={value} value={value}>{value}</option>)}</select></label>
              </div>
              <div className="flex justify-end gap-2">
                <Button variant="outline" disabled={!baseUrl.trim() || !model.trim() || testingAgent} onClick={() => void testAgent()}>{testingAgent ? <LoaderCircle className="animate-spin" /> : <PlugZap />}测试 Tool Calling</Button>
                <Button disabled={!baseUrl.trim() || !model.trim() || (!agent.configured && !apiKey.trim()) || savingAgent} onClick={() => void saveAgent()}>{savingAgent ? <LoaderCircle className="animate-spin" /> : <ShieldCheck />}保存模型设置</Button>
              </div>
            </div>
          </div>
        </div>

        <div className="border-b">
          <div className="mx-auto grid min-h-[96px] max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-4 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <h2 className="text-xs font-medium">外观</h2>
              <p className="mt-1 text-[10px] text-muted-foreground">应用主题</p>
            </div>
            <div className="grid grid-cols-3 gap-2">
              {themes.map(({ id, label, icon: Icon }) => (
                <button key={id} type="button" onClick={() => onThemeChange(id)} className={cn("flex h-12 items-center justify-center gap-2 rounded-md border bg-background text-[11px] outline-none transition hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring", theme === id && "border-primary/55 bg-primary/5 text-foreground ring-1 ring-primary/20")}>
                  <Icon className="size-3.5" />{label}
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="border-b">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-4 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <div className="flex items-center gap-2"><ShieldCheck className="size-3.5 text-muted-foreground" /><h2 className="text-xs font-medium">运行与隐私</h2></div>
              <p className="mt-2 text-[10px] text-muted-foreground">CPAH Docs {appVersion}</p>
            </div>
            <div className="space-y-2 text-[10px] leading-5 text-muted-foreground">
              <p>关闭主窗口后程序会留在 Windows 系统托盘或 macOS 菜单栏继续监控；请在图标菜单中选择“退出”来完全关闭。</p>
              <p>本地转换不会上传文件。使用 MinerU 时会把待解析文档发送到你配置的 MinerU 服务；开启 Agent 分类时会把 Markdown 内容发送到你配置的模型服务。</p>
              <p>MinerU Token 与 Agent API Key 均保存在系统凭据库（Windows 凭据管理器或 macOS 钥匙串）中，不写入设置文件。macOS 首次访问凭据时可能要求系统授权。</p>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
