import { useEffect, useState } from "react";
import { Bot, ExternalLink, KeyRound, Laptop, LoaderCircle, Moon, PlugZap, ShieldCheck, Sun } from "lucide-react";

import type { ThemeMode } from "@/app-model";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { AgentSettings, ClassificationModelType } from "@/types";

const modelOptions = [
  {
    value: "llm",
    title: "大语言模型",
    description: "通过工具读取文档、选择标签",
    help: "适合复杂文档和需要综合判断的分类，支持按需读取。多轮调用可能增加耗时和费用。",
  },
  {
    value: "decision",
    title: "决策模型",
    description: "根据文档直接判断候选标签",
    help: "适合类别明确、大批量的分类，有望提高效率、减少输出开销。复杂内容需要验证准确性。",
  },
] as const;

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
  onSaveAgent: (value: { modelType: ClassificationModelType; baseUrl: string; model: string; apiKey: string; concurrency: number }) => Promise<void>;
  onTestAgent: (value: { modelType: ClassificationModelType; baseUrl: string; model: string; apiKey: string }) => Promise<void>;
};

export function SettingsView({ appVersion, theme, onThemeChange, mineruConfigured, mineruBaseUrl, token, onTokenChange, savingToken, onSaveToken, onOpenMineruTokenPage, agent, onSaveAgent, onTestAgent }: SettingsViewProps) {
  const [modelType, setModelType] = useState(agent.modelType);
  const [baseUrl, setBaseUrl] = useState(agent.baseUrl);
  const [model, setModel] = useState(agent.model);
  const [apiKey, setApiKey] = useState("");
  const [concurrency, setConcurrency] = useState(agent.concurrency);
  const [savingAgent, setSavingAgent] = useState(false);
  const [testingAgent, setTestingAgent] = useState(false);
  useEffect(() => {
    setModelType(agent.modelType);
    setBaseUrl(agent.baseUrl);
    setModel(agent.model);
    setConcurrency(agent.concurrency);
  }, [agent.baseUrl, agent.concurrency, agent.model, agent.modelType]);
  const isDecision = modelType === "decision";

  async function saveAgent() {
    setSavingAgent(true);
    try {
      await onSaveAgent({ modelType, baseUrl, model, apiKey, concurrency });
      setApiKey("");
    } catch {
      // The parent displays the error; keep the draft available for correction.
    } finally {
      setSavingAgent(false);
    }
  }

  async function testAgent() {
    setTestingAgent(true);
    try {
      await onTestAgent({ modelType, baseUrl, model, apiKey });
    } catch {
      // The parent displays connection errors.
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
              <p className="mb-2 text-[10px] leading-4 text-muted-foreground">PDF 优先本地提取，仅需 OCR 时使用 MinerU；图片使用 MinerU。Token 由本机系统凭据库保存。</p>
              <div className="flex gap-2">
                <Input type="password" aria-label="MinerU Token" aria-describedby={mineruConfigured ? "mineru-token-saved" : undefined} autoComplete="new-password" value={token} onChange={(event) => onTokenChange(event.target.value)} placeholder={mineruConfigured ? "••••••••（已保存）" : "输入 MinerU Token"} />
                <Button disabled={!token.trim() || savingToken} onClick={onSaveToken}>{savingToken ? <LoaderCircle className="animate-spin" /> : <ShieldCheck />}{mineruConfigured ? "更新" : "保存"}</Button>
              </div>
              {mineruConfigured && <p id="mineru-token-saved" className="mt-2 text-[10px] text-muted-foreground">已保存 Token，输入新值可替换。</p>}
              <p className="mt-2 truncate text-[10px] text-muted-foreground">API 地址：{mineruBaseUrl}</p>
            </div>
          </div>
        </div>

        <div className="border-b">
          <div className="mx-auto grid max-w-[840px] grid-cols-[150px_minmax(0,1fr)] gap-5 px-5 py-4 max-[760px]:grid-cols-1 max-[900px]:px-4">
            <div>
              <div className="flex items-center gap-2"><Bot className="size-3.5 text-muted-foreground" /><h2 className="text-xs font-medium">分类模型</h2></div>
              <div className={cn("mt-2 inline-flex items-center gap-1.5 text-[10px]", agent.configured ? "text-success" : "text-muted-foreground")}><ShieldCheck className="size-3" />{agent.configured ? "API Key 已配置" : "尚未配置"}</div>
            </div>
            <div className="min-w-0 space-y-3">
              <div className="min-w-0 border-l-2 border-primary/35 pl-3">
                <p className="text-[11px] font-medium leading-5">用于文档标签分类</p>
                <p className="mt-0.5 break-words text-[10px] leading-4 text-muted-foreground">读取输出目录中的 Markdown，从监控目录配置的候选类别中选择，并把结果写入 YAML 的 <code className="break-all text-foreground">cpah_categories</code>。</p>
                <p className="mt-1 break-words text-[10px] leading-4 text-muted-foreground">只有目录已开启分类且“分类任务”正在运行时才会调用模型；不参与文档转换、目录同步、MinerU 解析或索引生成。</p>
              </div>
              <fieldset disabled={savingAgent || testingAgent} className="space-y-2">
                <legend className="mb-1 text-[10px] text-muted-foreground">模型类型</legend>
                <div className="grid grid-cols-2 gap-2 max-[580px]:grid-cols-1">
                  {modelOptions.map((option) => (
                    <Tooltip key={option.value}>
                      <TooltipTrigger asChild>
                        <label className={cn("flex cursor-pointer items-start gap-2 rounded-md border p-3 transition hover:bg-accent", modelType === option.value && "border-primary/55 bg-primary/5")}>
                          <input type="radio" name="classification-model-type" aria-label={option.title} aria-describedby={`model-help-${option.value}`} value={option.value} checked={modelType === option.value} onChange={() => setModelType(option.value)} className="mt-0.5 accent-primary" />
                          <span><span className="block text-[11px] font-medium">{option.title}</span><span className="mt-1 block text-[10px] leading-4 text-muted-foreground">{option.description}</span></span>
                          <span id={`model-help-${option.value}`} className="sr-only">{option.help}</span>
                        </label>
                      </TooltipTrigger>
                      <TooltipContent side="top" align="start" collisionPadding={12} className="max-w-[min(340px,calc(100vw-24px))] space-y-2 px-3 py-2.5 text-[11px] leading-5">
                        <p className="font-medium">{option.title}</p>
                        <p>{option.help}</p>
                      </TooltipContent>
                    </Tooltip>
                  ))}
                </div>
              </fieldset>
              <p className="break-words text-[10px] leading-4 text-muted-foreground">{isDecision ? "支持阿里百炼、Jev 等 System One 兼容接口。请填写对应服务的地址、模型名称和 API Key。" : "支持 OpenAI 兼容的 Chat Completions Tool Calling。"} API Key 只保存到系统凭据库。</p>
              <div className="grid grid-cols-[minmax(0,1fr)_180px] gap-2 max-[720px]:grid-cols-1">
                <label className="space-y-1"><span className="text-[10px] text-muted-foreground">Base URL</span><Input value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} placeholder={isDecision ? "https://你的业务空间ID.cn-beijing.maas.aliyuncs.com/compatible-mode/v1" : "https://api.openai.com/v1"} /></label>
                <label className="space-y-1"><span className="text-[10px] text-muted-foreground">模型名称</span><Input value={model} onChange={(event) => setModel(event.target.value)} placeholder={isDecision ? "decision-model-preview" : "gpt-4.1-mini"} /></label>
              </div>
              {isDecision && <p className="break-words text-[10px] leading-4 text-muted-foreground">百炼：填写业务空间的 Base URL，模型名称为 decision-model-preview。Jev：地址为 https://api.typesafe.ai/v1，模型名称为 jev-latest。也可粘贴完整的 /systemone 接口地址。</p>}
              <div className="grid grid-cols-[minmax(0,1fr)_120px] gap-2">
                <label className="space-y-1"><span className="text-[10px] text-muted-foreground">API Key</span><Input type="password" aria-describedby={agent.configured ? "agent-key-saved" : undefined} autoComplete="new-password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={agent.configured ? "••••••••（已保存）" : "输入 API Key"} /></label>
                <label className="space-y-1"><span className="text-[10px] text-muted-foreground">并发数</span><select value={concurrency} onChange={(event) => setConcurrency(Number(event.target.value))} className="h-8 w-full rounded-md border border-input bg-card px-2.5 text-xs outline-none focus:border-ring focus:ring-2 focus:ring-ring/20">{[1, 2, 3, 4].map((value) => <option key={value} value={value}>{value}</option>)}</select></label>
              </div>
              {agent.configured && <p id="agent-key-saved" className="text-[10px] text-muted-foreground">已保存 API Key，留空继续使用，输入新值可替换。</p>}
              <div className="flex justify-end gap-2">
                <Button variant="outline" disabled={!baseUrl.trim() || !model.trim() || (!agent.configured && !apiKey.trim()) || testingAgent || savingAgent} onClick={() => void testAgent()}>{testingAgent ? <LoaderCircle className="animate-spin" /> : <PlugZap />}{isDecision ? "测试决策模型" : "测试 Tool Calling"}</Button>
                <Button disabled={!baseUrl.trim() || !model.trim() || (!agent.configured && !apiKey.trim()) || savingAgent || testingAgent} onClick={() => void saveAgent()}>{savingAgent ? <LoaderCircle className="animate-spin" /> : <ShieldCheck />}保存模型设置</Button>
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
