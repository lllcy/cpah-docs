import { useEffect, useState } from "react";
import { Bot, ExternalLink, KeyRound, Laptop, LoaderCircle, Moon, PlugZap, ShieldCheck, Sun } from "lucide-react";

import type { ThemeMode } from "@/app-model";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import type { AgentSettings } from "@/types";

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
};

export function SettingsView({ appVersion, theme, onThemeChange, mineruConfigured, mineruBaseUrl, token, onTokenChange, savingToken, onSaveToken, onOpenMineruTokenPage, agent, onSaveAgent, onTestAgent }: SettingsViewProps) {
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
              <div className="flex items-center gap-2"><Bot className="size-3.5 text-muted-foreground" /><h2 className="text-xs font-medium">Agent 模型</h2></div>
              <div className={cn("mt-2 inline-flex items-center gap-1.5 text-[10px]", agent.configured ? "text-success" : "text-muted-foreground")}><ShieldCheck className="size-3" />{agent.configured ? "API Key 已配置" : "尚未配置"}</div>
            </div>
            <div className="min-w-0 space-y-3">
              <div className="min-w-0 border-l-2 border-primary/35 pl-3">
                <p className="text-[11px] font-medium leading-5">用于 Agent 文档分类</p>
                <p className="mt-0.5 break-words text-[10px] leading-4 text-muted-foreground">读取输出目录中的 Markdown，从监控目录配置的候选类别中选择，并把结果写入 YAML 的 <code className="break-all text-foreground">cpah_categories</code>。</p>
                <p className="mt-1 break-words text-[10px] leading-4 text-muted-foreground">只有目录已开启分类且“分类任务”正在运行时才会调用模型；不参与文档转换、目录同步、MinerU 解析或索引生成。</p>
              </div>
              <p className="break-words text-[10px] leading-4 text-muted-foreground">支持 OpenAI 兼容的 Chat Completions Tool Calling。API Key 只保存到 Windows 凭据管理器。</p>
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
              <p>关闭主窗口后程序会留在系统托盘继续监控；请在托盘菜单中选择“退出”来完全关闭。</p>
              <p>本地转换不会上传文件。使用 MinerU 时会把待解析文档发送到你配置的 MinerU 服务；开启 Agent 分类时会把 Markdown 内容发送到你配置的模型服务。</p>
              <p>MinerU Token 与 Agent API Key 均保存在当前 Windows 用户的凭据管理器中，不写入设置文件。</p>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
