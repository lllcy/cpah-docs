import { useEffect, useRef, useState } from "react";
import { ArrowDown, ArrowUp, ArrowUpRight, Bot, FolderInput, FolderOutput, LoaderCircle, Pause, Play, Plus, Save, Tags, Trash2, Upload } from "lucide-react";

import { IconAction } from "@/components/app/icon-action";
import { DirectoryFiles } from "@/components/app/directory-files";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { profileIsPersisted, type DirectorySaveState } from "@/app-model";
import { cn } from "@/lib/utils";
import { mergeCategoryImport, parseCategoryJson, previewCategoryImport, type ImportedCategory, type ImportPreview } from "@/tag-import";
import type { CategoryLabel, DeletePolicy, FileAction, FileEntry, TaggingConfig, TaggingImpact, TaskRecord, WatchProfile } from "@/types";

type DirectoriesViewProps = {
  profiles: WatchProfile[];
  persistedProfiles: WatchProfile[];
  selectedId: string | null;
  onSelectedIdChange: (id: string | null) => void;
  saving: boolean;
  removingProfileId: string | null;
  saveState: DirectorySaveState;
  saveError: string;
  monitoringPaused: boolean;
  changingMonitoringState: boolean;
  onToggleMonitoringPaused: () => void;
  onAdd: () => WatchProfile;
  onPatch: (id: string, patch: Partial<WatchProfile>) => void;
  onRemove: (id: string) => Promise<void>;
  onChooseDirectory: (id: string, field: "inputDir" | "outputDir") => void;
  onOpenDirectory: (path: string) => void;
  onSave: () => void;
  agentConfigured: boolean;
  onPreviewTagging: (profileId: string, tagging: TaggingConfig) => Promise<TaggingImpact>;
  onApplyTagging: (profileId: string, tagging: TaggingConfig, processExisting: boolean) => Promise<void>;
  onOpenTagTasks: () => void;
  onOpenSettings: () => void;
  enabledExtensions: string[];
  tasks: TaskRecord[];
  conversionPaused: boolean;
  classificationPaused: boolean;
  onFileAction: (profileId: string, entry: FileEntry, action: FileAction) => Promise<void>;
};

const categoryJsonExample = '[\n  {\n    "name": "PDF凭证",\n    "description": "PDF格式的会计记账凭证及附件。"\n  }\n]';

export function DirectoriesView({ profiles, persistedProfiles, selectedId, onSelectedIdChange, saving, removingProfileId, saveState, saveError, monitoringPaused, changingMonitoringState, onToggleMonitoringPaused, onAdd, onPatch, onRemove, onChooseDirectory, onOpenDirectory, onSave, agentConfigured, onPreviewTagging, onApplyTagging, onOpenTagTasks, onOpenSettings, enabledExtensions, tasks, conversionPaused, classificationPaused, onFileAction }: DirectoriesViewProps) {
  const importButtonRef = useRef<HTMLButtonElement>(null);
  const importDialogRef = useRef<HTMLDialogElement>(null);
  const conflictDialogRef = useRef<HTMLDialogElement>(null);
  const selectedIdRef = useRef(selectedId);
  const profilesRef = useRef(profiles);
  const [tabs, setTabs] = useState<Record<string, "files" | "settings">>({});
  const [impact, setImpact] = useState<TaggingImpact | null>(null);
  const [checkingImpact, setCheckingImpact] = useState(false);
  const [applyingTags, setApplyingTags] = useState(false);
  const [confirmRemoveId, setConfirmRemoveId] = useState<string | null>(null);
  const [editingImportProfileId, setEditingImportProfileId] = useState<string | null>(null);
  const [importDraft, setImportDraft] = useState("");
  const [importError, setImportError] = useState("");
  const [importFeedback, setImportFeedback] = useState<{ kind: "error" | "success"; text: string } | null>(null);
  const [pendingImport, setPendingImport] = useState<{ profileId: string; imported: ImportedCategory[]; preview: ImportPreview } | null>(null);
  selectedIdRef.current = selectedId;
  profilesRef.current = profiles;
  useEffect(() => {
    setImpact(null);
    setConfirmRemoveId(null);
    setPendingImport(null);
    setEditingImportProfileId(null);
    setImportDraft("");
    setImportError("");
    setImportFeedback(null);
  }, [selectedId]);
  useEffect(() => {
    if (editingImportProfileId === selectedId && importDialogRef.current && !importDialogRef.current.open) {
      importDialogRef.current.showModal();
    }
  }, [editingImportProfileId, selectedId]);
  useEffect(() => {
    if (pendingImport && conflictDialogRef.current && !conflictDialogRef.current.open) {
      conflictDialogRef.current.showModal();
    }
  }, [pendingImport]);
  const selected = profiles.find((profile) => profile.id === selectedId) ?? null;
  const activeImport = pendingImport?.profileId === selectedId ? pendingImport : null;
  const persistedSelected = persistedProfiles.find((profile) => profile.id === selectedId) ?? null;
  const tab = selected ? tabs[selected.id] ?? (persistedSelected ? "files" : "settings") : "settings";
  useEffect(() => {
    if (!selectedId) return;
    setTabs((current) => current[selectedId] ? current : { ...current, [selectedId]: persistedSelected ? "files" : "settings" });
  }, [selectedId, Boolean(persistedSelected)]);
  const directoryPersisted = Boolean(selected && persistedSelected
    && selected.name === persistedSelected.name
    && selected.inputDir === persistedSelected.inputDir
    && selected.outputDir === persistedSelected.outputDir
    && selected.enabled === persistedSelected.enabled
    && selected.deletePolicy === persistedSelected.deletePolicy);
  const taggingPersisted = Boolean(selected && persistedSelected
    && JSON.stringify(selected.tagging) === JSON.stringify(persistedSelected.tagging));
  const categoryNames = selected?.tagging.labels.map((label) => label.name.trim()) ?? [];
  const rulesReady = Boolean(selected
    && categoryNames.every((name) => name && name !== "未分类")
    && new Set(categoryNames.map((name) => name.toLocaleLowerCase())).size === categoryNames.length
    && (!selected.tagging.enabled || categoryNames.length > 0));
  const saveLabel = removingProfileId ? "正在删除监控目录…" : ({
    saved: "所有更改已自动保存",
    dirty: "即将自动保存…",
    incomplete: "填写完整后自动保存",
    saving: "正在自动保存…",
    error: "自动保存失败，可手动重试",
  }[saveState]);

  function addProfile() {
    const profile = onAdd();
    setTabs((current) => ({ ...current, [profile.id]: "settings" }));
    onSelectedIdChange(profile.id);
  }

  function patchTagging(patch: Partial<TaggingConfig>) {
    if (!selected) return;
    setImpact(null);
    onPatch(selected.id, { tagging: { ...selected.tagging, ...patch } });
  }

  function addCategoryLabel() {
    patchTagging({ labels: [...selected!.tagging.labels, { id: crypto.randomUUID(), name: "", description: "" }] });
  }

  function patchCategoryLabel(id: string, patch: Partial<CategoryLabel>) {
    patchTagging({ labels: selected!.tagging.labels.map((label) => label.id === id ? { ...label, ...patch } : label) });
  }

  function moveCategoryLabel(index: number, offset: number) {
    const labels = [...selected!.tagging.labels];
    const target = index + offset;
    if (target < 0 || target >= labels.length) return;
    [labels[index], labels[target]] = [labels[target], labels[index]];
    patchTagging({ labels });
  }

  function finishCategoryImport(imported: ImportedCategory[], choice: "keep" | "overwrite" | "cancel") {
    const current = profilesRef.current.find((profile) => profile.id === selectedIdRef.current);
    if (!current) return;
    try {
      const result = mergeCategoryImport(current.tagging.labels, imported, choice, () => crypto.randomUUID());
      if (!result) return;
      if (result.added || result.overwritten) {
        setImpact(null);
        onPatch(current.id, { tagging: { ...current.tagging, labels: result.labels } });
      }
      setImportFeedback({ kind: "success", text: `导入完成：新增 ${result.added} 项，跳过 ${result.skipped} 项，覆盖说明 ${result.overwritten} 项。` });
    } catch (error) {
      setImportFeedback({ kind: "error", text: error instanceof Error ? error.message : "导入失败，请重试。" });
    }
  }

  function openImportEditor() {
    if (!selected) return;
    setImportDraft("");
    setImportError("");
    setImportFeedback(null);
    setEditingImportProfileId(selected.id);
  }

  function closeImportEditor(restoreFocus = true) {
    importDialogRef.current?.close();
    setEditingImportProfileId(null);
    setImportDraft("");
    setImportError("");
    if (restoreFocus) importButtonRef.current?.focus();
  }

  function submitCategoryJson() {
    if (!selected || editingImportProfileId !== selected.id) return;
    try {
      const imported = parseCategoryJson(importDraft);
      const current = profilesRef.current.find((profile) => profile.id === selected.id);
      if (!current) return;
      const preview = previewCategoryImport(current.tagging.labels, imported);
      if (preview.conflicts.length) {
        closeImportEditor(false);
        setPendingImport({ profileId: current.id, imported, preview });
      } else {
        finishCategoryImport(imported, "keep");
        closeImportEditor();
      }
    } catch (error) {
      setImportError(error instanceof Error ? error.message : "导入失败，请检查 JSON。");
    }
  }

  function closeConflictDialog() {
    conflictDialogRef.current?.close();
    setPendingImport(null);
    importButtonRef.current?.focus();
  }

  function resolveConflict(choice: "keep" | "overwrite" | "cancel") {
    if (pendingImport && selected && pendingImport.profileId === selected.id) {
      finishCategoryImport(pendingImport.imported, choice);
    }
    closeConflictDialog();
  }

  async function previewTagging() {
    if (!selected) return;
    setCheckingImpact(true);
    try {
      setImpact(await onPreviewTagging(selected.id, selected.tagging));
    } finally {
      setCheckingImpact(false);
    }
  }

  async function applyTagging(processExisting: boolean) {
    if (!selected) return;
    setApplyingTags(true);
    try {
      await onApplyTagging(selected.id, selected.tagging, processExisting);
      setImpact(null);
    } finally {
      setApplyingTags(false);
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-[62px] shrink-0 items-center border-b px-5 max-[900px]:px-4">
        <div>
          <h1 className="text-[15px] font-semibold tracking-[-0.01em]">监控目录</h1>
          <p className="mt-0.5 text-[11px] text-muted-foreground">浏览源文件，查看转换进度与目录设置</p>
        </div>
        <div className="ml-auto flex min-w-0 items-center gap-2">
          <span className={cn("max-w-40 truncate text-[10px] max-[980px]:hidden", saveState === "error" ? "text-destructive" : "text-muted-foreground")} title={saveError || saveLabel}>{saveLabel}</span>
          <Button variant={monitoringPaused ? "default" : "outline"} size="sm" disabled={changingMonitoringState} onClick={onToggleMonitoringPaused}>
            {changingMonitoringState ? <LoaderCircle className="animate-spin" /> : monitoringPaused ? <Play /> : <Pause />}
            {monitoringPaused ? "开始监听" : "停止监听"}
          </Button>
          <Button variant="outline" size="sm" onClick={addProfile}><Plus />添加目录</Button>
          <Button size="sm" disabled={saving || saveState === "saved"} onClick={onSave}>{saving ? <LoaderCircle className="animate-spin" /> : <Save />}{saving ? "保存中" : saveState === "saved" ? "已保存" : "立即保存"}</Button>
        </div>
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-[220px_minmax(0,1fr)] max-[860px]:grid-cols-[190px_minmax(0,1fr)]">
        <aside className="min-h-0 overflow-y-auto border-r bg-[var(--sidebar)] p-2">
          {profiles.length === 0 ? (
            <div className="px-4 py-10 text-center">
              <FolderInput className="mx-auto mb-3 size-5 text-muted-foreground/50" />
              <p className="text-xs font-medium">没有监控目录</p>
              <p className="mt-1 text-[11px] leading-5 text-muted-foreground">添加一个输入目录和对应的输出目录。</p>
            </div>
          ) : profiles.map((profile) => {
            const persisted = profileIsPersisted(profile, persistedProfiles);
            return (
            <button key={profile.id} type="button" onClick={() => onSelectedIdChange(profile.id)} className={cn("mb-0.5 flex min-h-12 w-full items-center gap-2.5 rounded-md px-2.5 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring", selectedId === profile.id ? "bg-[var(--selection)] shadow-[inset_0_0_0_1px_color-mix(in_srgb,var(--border)_60%,transparent)]" : "hover:bg-accent") }>
              <span className={cn("size-1.5 shrink-0 rounded-full", !persisted ? "bg-amber-500" : profile.enabled ? "bg-success" : "bg-muted-foreground/45")} />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-xs font-medium">{profile.name}</span>
                <span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{profile.inputDir || "未选择目录"}</span>
              </span>
              {removingProfileId === profile.id
                ? <span className="inline-flex shrink-0 items-center gap-1 text-[9px] text-destructive"><LoaderCircle className="size-3 animate-spin" />正在删除</span>
                : !persisted && <span className="shrink-0 text-[9px] text-amber-600 dark:text-amber-400">待保存</span>}
            </button>
          );})}
        </aside>

        <section className="flex min-h-0 min-w-0 flex-col bg-[var(--inspector)]">
          {selected && <div className="shrink-0 border-b bg-background px-4 pt-4">
            <p className="flex items-center gap-2 truncate text-xs font-semibold"><FolderInput className="size-3.5 shrink-0 text-primary" />{selected.name}</p>
            <p className="mt-1 truncate text-[11px] text-muted-foreground" title={tab === "files" ? persistedSelected?.inputDir : selected.inputDir}>{tab === "files" ? persistedSelected?.inputDir : selected.inputDir || "尚未选择输入目录"}</p>
            <div role="tablist" aria-label="监控目录内容" className="mt-3 flex gap-6">
              {([{ id: "files", label: "文件" }, { id: "settings", label: "目录设置" }] as const).map((item) => <button key={item.id} id={`directory-${item.id}-tab`} role="tab" aria-selected={tab === item.id} aria-controls={`directory-${item.id}-panel`} disabled={item.id === "files" && !persistedSelected} type="button" onClick={() => setTabs((current) => ({ ...current, [selected.id]: item.id }))} className={cn("border-b-2 border-transparent pb-2 text-[11px] outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40", tab === item.id ? "border-primary font-medium text-primary" : "text-muted-foreground")}>{item.label}</button>)}
            </div>
          </div>}
          {selected && tab === "files" && persistedSelected ? <div role="tabpanel" id="directory-files-panel" aria-labelledby="directory-files-tab" className="flex min-h-0 flex-1 flex-col">
            {selected.inputDir !== persistedSelected.inputDir && <p className="shrink-0 border-b px-4 py-2 text-[11px] text-muted-foreground">路径更改尚未保存，当前显示已保存目录中的文件。</p>}
            <DirectoryFiles key={`${persistedSelected.id}:${persistedSelected.inputDir}:${persistedSelected.outputDir}`} profile={persistedSelected} enabledExtensions={enabledExtensions} tasks={tasks} conversionPaused={conversionPaused} monitoringPaused={monitoringPaused} classificationPaused={classificationPaused} agentConfigured={agentConfigured} onOpen={onOpenDirectory} onFileAction={onFileAction} />
          </div> : <div role="tabpanel" id="directory-settings-panel" aria-labelledby="directory-settings-tab" className="min-h-0 flex-1 overflow-y-auto">
          {selected ? (
            <div className="mx-auto max-w-[720px] px-6 py-6 max-[900px]:px-4">
              <div className="mb-6 flex items-start gap-4 border-b pb-5">
                <div className="min-w-0 flex-1">
                  <label className="mb-1.5 block text-[10px] font-medium text-muted-foreground" htmlFor={`name-${selected.id}`}>目录名称</label>
                  <Input id={`name-${selected.id}`} value={selected.name} onChange={(event) => onPatch(selected.id, { name: event.target.value })} className="max-w-sm" />
                </div>
                <div className="flex items-center gap-2 pt-5 text-[11px] text-muted-foreground">
                  <span>{selected.enabled ? "已启用" : "已停用"}</span>
                  <Switch checked={selected.enabled} onCheckedChange={(enabled) => onPatch(selected.id, { enabled })} aria-label="启用监控目录" />
                </div>
                <div className="flex items-center gap-1 pt-4">
                  {confirmRemoveId === selected.id ? (
                    <>
                      <span className="max-w-40 text-right text-[10px] leading-4 text-destructive">移除配置和任务记录，不删除源文件或输出文件</span>
                      <Button variant="ghost" size="sm" disabled={removingProfileId === selected.id} onClick={() => setConfirmRemoveId(null)}>取消</Button>
                      <Button variant="destructive" size="sm" disabled={saving || removingProfileId === selected.id} onClick={() => void onRemove(selected.id)}>
                        {removingProfileId === selected.id && <LoaderCircle className="animate-spin" />}
                        {removingProfileId === selected.id ? "正在删除" : "确认删除"}
                      </Button>
                    </>
                  ) : (
                    <IconAction label="删除监控目录" variant="destructive" onClick={() => setConfirmRemoveId(selected.id)}><Trash2 /></IconAction>
                  )}
                </div>
              </div>

              <div className="space-y-5">
                <div>
                  <label className="mb-1.5 block text-[10px] font-medium text-muted-foreground">① 监控目录（原始文档）</label>
                  <div className="flex gap-2">
                    <div className="relative min-w-0 flex-1"><FolderInput className="absolute left-2.5 top-2 size-3.5 text-muted-foreground" /><Input value={selected.inputDir} onChange={(event) => onPatch(selected.id, { inputDir: event.target.value })} className="pl-8" placeholder="选择需要自动转换的目录" /></div>
                    <Button variant="outline" onClick={() => onChooseDirectory(selected.id, "inputDir")}>选择</Button>
                  </div>
                  <p className="mt-1.5 text-[10px] leading-4 text-muted-foreground">会递归监控此目录中的受支持文档与子目录。</p>
                </div>

                <div>
                  <label className="mb-1.5 block text-[10px] font-medium text-muted-foreground">② 输出目录（Markdown）</label>
                  <div className="flex gap-2">
                    <div className="relative min-w-0 flex-1"><FolderOutput className="absolute left-2.5 top-2 size-3.5 text-muted-foreground" /><Input value={selected.outputDir} onChange={(event) => onPatch(selected.id, { outputDir: event.target.value })} className="pl-8" placeholder="选择 Markdown 输出目录" /></div>
                    <Button variant="outline" onClick={() => onChooseDirectory(selected.id, "outputDir")}>选择</Button>
                    <IconAction label="打开输出目录" variant="outline" disabled={!selected.outputDir} onClick={() => selected.outputDir && onOpenDirectory(selected.outputDir)}><ArrowUpRight /></IconAction>
                  </div>
                  <p className="mt-1.5 text-[10px] leading-4 text-muted-foreground">请在选择窗口、Finder 或文件资源管理器中新建一个专用文件夹。它不能与监控目录相同或互相包含；输出会保留原目录结构、生成同名 `.md`，并在根目录自动维护 `index.md`。</p>
                </div>

                <div className="border-t pt-5">
                  <label className="mb-1.5 block text-[10px] font-medium text-muted-foreground" htmlFor={`policy-${selected.id}`}>源文件删除策略</label>
                  <select id={`policy-${selected.id}`} value={selected.deletePolicy} onChange={(event) => onPatch(selected.id, { deletePolicy: event.target.value as DeletePolicy })} className="h-8 w-full max-w-sm rounded-md border border-input bg-card px-2.5 text-xs outline-none focus:border-ring focus:ring-2 focus:ring-ring/20">
                    <option value="trash">移动到输出目录的 .trash（可恢复）</option>
                    <option value="keep">保留转换结果</option>
                    <option value="delete">永久删除对应结果</option>
                  </select>
                </div>

                <div className="border-t pt-5">
                  <div className="flex items-start gap-4">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2"><Tags className="size-3.5 text-muted-foreground" /><h3 className="text-xs font-medium">Agent 文档分类</h3></div>
                    </div>
                    <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
                      <Button ref={importButtonRef} variant="outline" size="sm" disabled={removingProfileId === selected.id} onClick={openImportEditor}><Upload />导入 JSON</Button>
                      <span>{selected.tagging.enabled ? "已开启" : "已关闭"}</span><Switch checked={selected.tagging.enabled} onCheckedChange={(enabled) => patchTagging({ enabled })} aria-label="开启 Agent 文档分类" />
                    </div>
                  </div>

                  {importFeedback && <p role={importFeedback.kind === "error" ? "alert" : "status"} className={cn("mt-2 rounded-md border px-3 py-2 text-[10px] leading-4", importFeedback.kind === "error" ? "border-destructive/30 text-destructive" : "border-success/30 text-foreground")}>{importFeedback.text}</p>}

                  {(selected.tagging.enabled || selected.tagging.labels.length > 0) && (
                    <div className="mt-4 space-y-3">
                      {selected.tagging.enabled && !agentConfigured && <div className="flex items-center gap-2 rounded-md border border-amber-500/30 bg-amber-500/5 px-3 py-2 text-[10px] text-amber-700 dark:text-amber-300"><Bot className="size-3.5" /><span>分类规则可以先保存；开始分类前需要配置 Agent 模型。</span><Button className="ml-auto h-6 px-2 text-[10px]" variant="ghost" size="sm" onClick={onOpenSettings}>前往设置</Button></div>}
                      {selected.tagging.enabled && <div>
                        <label className="mb-1.5 block text-[10px] font-medium text-muted-foreground">分类模式</label>
                        <select value={selected.tagging.selectionMode} onChange={(event) => patchTagging({ selectionMode: event.target.value as TaggingConfig["selectionMode"] })} className="h-8 w-full max-w-sm rounded-md border border-input bg-card px-2.5 text-xs outline-none focus:border-ring focus:ring-2 focus:ring-ring/20"><option value="single">单分类 · 每篇只选择一个类别</option><option value="multiple">多分类 · 每篇可选择多个类别</option></select>
                      </div>}
                      <div className="pt-1"><p className="text-[10px] font-medium text-muted-foreground">候选类别</p><p className="mt-1 text-[10px] leading-4 text-muted-foreground">Agent 只能从这里选择；没有适合类别时自动使用“未分类”。</p></div>
                      {selected.tagging.labels.map((label, index) => (
                        <div key={label.id} className="rounded-md border bg-background p-3">
                          <div className="grid grid-cols-[minmax(130px,0.7fr)_minmax(220px,1.3fr)_auto] gap-2 max-[820px]:grid-cols-1">
                            <Input value={label.name} onChange={(event) => patchCategoryLabel(label.id, { name: event.target.value })} placeholder="类别名称，如培训材料" />
                            <Input value={label.description} onChange={(event) => patchCategoryLabel(label.id, { description: event.target.value })} placeholder="判断说明（可选），如课程、讲义和培训案例" />
                            <div className="flex gap-1">
                              <IconAction label="上移类别" size="icon-sm" variant="outline" disabled={index === 0} onClick={() => moveCategoryLabel(index, -1)}><ArrowUp /></IconAction>
                              <IconAction label="下移类别" size="icon-sm" variant="outline" disabled={index === selected.tagging.labels.length - 1} onClick={() => moveCategoryLabel(index, 1)}><ArrowDown /></IconAction>
                              <IconAction label="删除类别" size="icon-sm" variant="destructive" onClick={() => patchTagging({ labels: selected.tagging.labels.filter((item) => item.id !== label.id) })}><Trash2 /></IconAction>
                            </div>
                          </div>
                        </div>
                      ))}
                      <Button variant="outline" size="sm" onClick={addCategoryLabel}><Plus />添加候选类别</Button>
                    </div>
                  )}

                  <div className="mt-4 flex flex-wrap items-center justify-between gap-2 border-t pt-4">
                    <span className={cn("text-[10px]", taggingPersisted ? "text-muted-foreground" : "text-amber-600 dark:text-amber-400")}>{taggingPersisted ? "分类规则已保存" : rulesReady ? "分类规则即将自动保存" : "请先完整填写分类规则"}</span>
                    <div className="flex flex-wrap justify-end gap-2">
                      {selected.tagging.enabled && <Button variant="ghost" size="sm" disabled={!rulesReady || !directoryPersisted || checkingImpact || applyingTags} onClick={() => void previewTagging()}>{checkingImpact ? <LoaderCircle className="animate-spin" /> : null}预览影响</Button>}
                      <Button variant="outline" size="sm" disabled={!rulesReady || !directoryPersisted || taggingPersisted || applyingTags} onClick={() => void applyTagging(false)}>{applyingTags ? <LoaderCircle className="animate-spin" /> : <Save />}保存分类规则</Button>
                      {selected.tagging.enabled && <Button size="sm" onClick={onOpenTagTasks}><ArrowUpRight />前往分类任务</Button>}
                    </div>
                  </div>

                  {impact && selected.tagging.enabled && (
                    <div className="mt-3 rounded-md border border-primary/25 bg-primary/5 p-3">
                      <p className="text-[11px] font-medium">输出目录共发现 {impact.discovered} 篇 Markdown</p>
                      <p className="mt-1 text-[10px] leading-4 text-muted-foreground">其中 {impact.newFiles} 篇尚未建立分类任务，按当前规则有 {impact.affected} 篇需要处理。保存规则后请前往“分类任务”页面统一开始或处理待分类项。</p>
                    </div>
                  )}
                  {editingImportProfileId === selected.id && (
                    <dialog ref={importDialogRef} aria-labelledby="category-json-title" onCancel={(event) => { event.preventDefault(); closeImportEditor(); }} className="m-auto max-h-[80vh] w-[min(600px,calc(100vw-32px))] rounded-lg border bg-card p-5 text-foreground shadow-xl backdrop:bg-black/50">
                      <h3 id="category-json-title" className="text-sm font-semibold">导入候选类别 JSON</h3>
                      <label htmlFor="category-json-input" className="mt-4 block text-xs text-muted-foreground">粘贴 JSON</label>
                      <textarea id="category-json-input" autoFocus value={importDraft} onChange={(event) => { setImportDraft(event.target.value); setImportError(""); }} placeholder={categoryJsonExample} spellCheck={false} className="mt-2 min-h-56 max-h-[48vh] w-full resize-y rounded-md border border-input bg-background p-3 font-mono text-xs leading-5 outline-none focus:border-ring focus:ring-2 focus:ring-ring/20" />
                      {importError && <p role="alert" className="mt-2 text-xs leading-5 text-destructive">{importError}</p>}
                      <div className="mt-4 flex justify-end gap-2">
                        <Button variant="ghost" onClick={() => closeImportEditor()}>取消</Button>
                        <Button disabled={!importDraft.trim()} onClick={submitCategoryJson}>导入</Button>
                      </div>
                    </dialog>
                  )}
                  {activeImport && (
                    <dialog ref={conflictDialogRef} aria-labelledby="category-import-title" onCancel={(event) => { event.preventDefault(); resolveConflict("cancel"); }} className="m-auto max-h-[80vh] w-[min(560px,calc(100vw-32px))] rounded-lg border bg-card p-5 text-foreground shadow-xl backdrop:bg-black/50">
                      <h3 id="category-import-title" className="text-sm font-semibold">确认类别说明冲突</h3>
                      <p className="mt-2 text-xs leading-5 text-muted-foreground">发现 {activeImport.preview.conflicts.length} 个同名但说明不同的类别。以下选择会统一应用于全部冲突；另有 {activeImport.preview.added} 个新类别将追加，{activeImport.preview.identical} 个完全相同的类别将跳过。</p>
                      <div className="mt-4 max-h-[45vh] space-y-2 overflow-y-auto">
                        {activeImport.preview.conflicts.map((conflict) => (
                          <div key={conflict.name} className="rounded-md border p-3 text-xs leading-5">
                            <p className="font-medium">{conflict.name}</p>
                            <p className="mt-1 break-words text-muted-foreground">原说明：{conflict.currentDescription || "（空）"}</p>
                            <p className="break-words text-muted-foreground">新说明：{conflict.importedDescription || "（空）"}</p>
                          </div>
                        ))}
                      </div>
                      <div className="mt-5 flex flex-wrap justify-end gap-2">
                        <Button variant="ghost" onClick={() => resolveConflict("cancel")}>取消导入</Button>
                        <Button variant="outline" onClick={() => resolveConflict("keep")}>全部保留原说明</Button>
                        <Button onClick={() => resolveConflict("overwrite")}>全部覆盖说明</Button>
                      </div>
                    </dialog>
                  )}
                </div>
              </div>
            </div>
          ) : (
            <div className="flex h-full flex-col items-center justify-center px-6 text-center">
              <FolderOutput className="mb-3 size-5 text-muted-foreground/50" />
              <p className="text-xs font-medium">选择或添加监控目录</p>
            </div>
          )}
          </div>}
        </section>
      </div>
    </div>
  );
}
