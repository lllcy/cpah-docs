import { t } from "./i18n.ts";
export type ImportedCategory = {
  name: string;
  description: string;
};

export type ExistingCategory = ImportedCategory & { id: string };

export type CategoryConflict = {
  name: string;
  currentDescription: string;
  importedDescription: string;
};

export type ImportPreview = {
  added: number;
  identical: number;
  conflicts: CategoryConflict[];
};

export type ImportResult = {
  labels: ExistingCategory[];
  added: number;
  skipped: number;
  overwritten: number;
};

function normalizedName(name: string): string {
  return name.trim().toLowerCase();
}

export function parseCategoryJson(text: string): ImportedCategory[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    const detail = error instanceof Error ? error.message : "";
    const lineColumn = detail.match(/line\s+(\d+)\s+column\s+(\d+)/i);
    const offset = detail.match(/position\s+(\d+)/i);
    const location = lineColumn
      ? t("第 {p0} 行第 {p1} 列", { p0: lineColumn[1], p1: lineColumn[2] })
      : offset ? t("第 {p0} 个字符", { p0: Number(offset[1]) + 1 }) : t("文件开头");
    throw new Error(t("JSON 格式无效（{p0}），请提供纯 JSON 数组，不要包含 Markdown 代码围栏。", { p0: location }));
  }
  if (!Array.isArray(parsed) || parsed.length === 0) {
    throw new Error(t("JSON 顶层必须是非空数组。"));
  }

  const seen = new Set<string>();
  return parsed.map((entry: unknown, index: number) => {
    const position = index + 1;
    if (entry === null || typeof entry !== "object" || Array.isArray(entry)) {
      throw new Error(t("第 {p0} 项必须是包含 name 和 description 的对象。", { p0: position }));
    }
    const fields = Object.keys(entry);
    if (fields.length !== 2 || !fields.includes("name") || !fields.includes("description")) {
      throw new Error(t("第 {p0} 项只能包含 name 和 description；旧字段 value 不受支持。", { p0: position }));
    }
    const { name, description } = entry as Record<string, unknown>;
    if (typeof name !== "string" || !name.trim()) {
      throw new Error(t("第 {p0} 项的 name 必须是非空字符串。", { p0: position }));
    }
    if (typeof description !== "string") {
      throw new Error(t("第 {p0} 项的 description 必须是字符串，可以为空。", { p0: position }));
    }
    const trimmedName = name.trim();
    if (trimmedName === "未分类") {
      throw new Error(t("第 {p0} 项的“未分类”是系统保留类别。", { p0: position }));
    }
    const key = normalizedName(trimmedName);
    if (seen.has(key)) {
      throw new Error(t("第 {p0} 项的类别名称“{p1}”在文件中重复。", { p0: position, p1: trimmedName }));
    }
    seen.add(key);
    return { name: trimmedName, description: description.trim() };
  });
}

function existingByName(existing: readonly ExistingCategory[]): Map<string, ExistingCategory> {
  const result = new Map<string, ExistingCategory>();
  for (const label of existing) {
    const key = normalizedName(label.name);
    if (!key || label.name.trim() === "未分类" || result.has(key)) {
      throw new Error(t("当前候选类别有空名称、重复名称或保留名称，请先修正后再导入。"));
    }
    result.set(key, label);
  }
  return result;
}

export function previewCategoryImport(
  existing: readonly ExistingCategory[],
  imported: readonly ImportedCategory[],
): ImportPreview {
  const byName = existingByName(existing);
  const conflicts: CategoryConflict[] = [];
  let added = 0;
  let identical = 0;
  for (const candidate of imported) {
    const current = byName.get(normalizedName(candidate.name));
    if (!current) {
      added += 1;
    } else if (current.description.trim() === candidate.description) {
      identical += 1;
    } else {
      conflicts.push({
        name: current.name,
        currentDescription: current.description,
        importedDescription: candidate.description,
      });
    }
  }
  return { added, identical, conflicts };
}

export function mergeCategoryImport(
  existing: readonly ExistingCategory[],
  imported: readonly ImportedCategory[],
  conflictChoice: "keep" | "overwrite" | "cancel",
  createId: () => string,
): ImportResult | null {
  if (conflictChoice === "cancel") return null;
  const preview = previewCategoryImport(existing, imported);
  const labels = existing.map((label) => ({ ...label }));
  const indexByName = new Map(labels.map((label, index) => [normalizedName(label.name), index]));
  let overwritten = 0;
  for (const candidate of imported) {
    const index = indexByName.get(normalizedName(candidate.name));
    if (index === undefined) {
      indexByName.set(normalizedName(candidate.name), labels.length);
      labels.push({ id: createId(), ...candidate });
    } else if (conflictChoice === "overwrite" && labels[index].description.trim() !== candidate.description) {
      labels[index].description = candidate.description;
      overwritten += 1;
    }
  }
  return {
    labels,
    added: preview.added,
    skipped: preview.identical + preview.conflicts.length - overwritten,
    overwritten,
  };
}
