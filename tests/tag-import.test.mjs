import assert from "node:assert/strict";
import test from "node:test";
import { setLanguagePreference } from "../src/i18n.ts";

setLanguagePreference("zh-CN");

import { mergeCategoryImport, parseCategoryJson, previewCategoryImport } from "../src/tag-import.ts";

const existing = [
  { id: "old-a", name: "PDF凭证", description: "原说明" },
  { id: "old-b", name: "合同", description: "合同文件" },
];

test("直接粘贴的多行 JSON 使用 name/description 结构", () => {
  const pasted = '[\n  {\n    "name": "PDF凭证",\n    "description": "PDF格式的会计记账凭证及附件。"\n  }\n]';
  assert.deepEqual(parseCategoryJson(pasted), [
    { name: "PDF凭证", description: "PDF格式的会计记账凭证及附件。" },
  ]);
});

test("纯 name/description 数组可批量解析，名称和说明修剪空白", () => {
  const source = Array.from({ length: 157 }, (_, index) => ({
    name: ` 类别 ${index + 1} `,
    description: index === 0 ? "" : ` 说明 ${index + 1} `,
  }));
  const parsed = parseCategoryJson(JSON.stringify(source));
  assert.equal(parsed.length, 157);
  assert.deepEqual(parsed[0], { name: "类别 1", description: "" });
  assert.deepEqual(parsed[156], { name: "类别 157", description: "说明 157" });
});

test("格式错误整批拒绝，指出具体条目且不接受旧 value 字段", () => {
  assert.throws(() => parseCategoryJson('```json\n[]\n```'), /JSON 格式无效（文件开头）.*纯 JSON 数组/);
  assert.throws(() => parseCategoryJson("{}"), /顶层必须是非空数组/);
  assert.throws(() => parseCategoryJson("[]"), /顶层必须是非空数组/);
  const valid = { name: "正常", description: "" };
  for (const invalid of [
    null,
    { name: "旧格式", value: "不支持" },
    { name: "多余字段", description: "", id: "old-id" },
    { name: "", description: "" },
    { name: 123, description: "" },
    { name: "未分类", description: "" },
    { name: "说明类型错", description: null },
    { name: "缺说明" },
  ]) {
    assert.throws(() => parseCategoryJson(JSON.stringify([valid, invalid])), /第 2 项/);
  }
});

test("文件内重复名称忽略两端空白和大小写", () => {
  assert.throws(
    () => parseCategoryJson(JSON.stringify([
      { name: "PDF凭证", description: "A" },
      { name: " pdf凭证 ", description: "B" },
    ])),
    /第 2 项.*重复/,
  );
});

test("预览统计新增、相同和冲突，不修改原类别", () => {
  const snapshot = structuredClone(existing);
  const imported = parseCategoryJson(JSON.stringify([
    { name: "pdf凭证", description: "原说明" },
    { name: "合同", description: "新说明" },
    { name: "审计报告", description: "报告" },
  ]));
  const preview = previewCategoryImport(existing, imported);
  assert.deepEqual(preview, {
    added: 1,
    identical: 1,
    conflicts: [{ name: "合同", currentDescription: "合同文件", importedDescription: "新说明" }],
  });
  assert.deepEqual(existing, snapshot);
});

test("全部保留原说明：追加新类别，冲突和相同项跳过", () => {
  const imported = parseCategoryJson(JSON.stringify([
    { name: "PDF凭证", description: "原说明" },
    { name: "合同", description: "新说明" },
    { name: "审计报告", description: "报告" },
  ]));
  const result = mergeCategoryImport(existing, imported, "keep", () => "new-id");
  assert.deepEqual(result, {
    labels: [...existing, { id: "new-id", name: "审计报告", description: "报告" }],
    added: 1,
    skipped: 2,
    overwritten: 0,
  });
});

test("全部覆盖说明：保留旧 ID、名称、顺序，不改动源数据", () => {
  const snapshot = structuredClone(existing);
  const imported = parseCategoryJson(JSON.stringify([
    { name: "pdf凭证", description: "更新 A" },
    { name: "合同", description: "更新 B" },
    { name: "审计报告", description: "报告" },
  ]));
  const result = mergeCategoryImport(existing, imported, "overwrite", () => "new-id");
  assert.deepEqual(result, {
    labels: [
      { id: "old-a", name: "PDF凭证", description: "更新 A" },
      { id: "old-b", name: "合同", description: "更新 B" },
      { id: "new-id", name: "审计报告", description: "报告" },
    ],
    added: 1,
    skipped: 0,
    overwritten: 2,
  });
  assert.deepEqual(existing, snapshot);
});

test("取消冲突：不生成 ID，也不修改原类别", () => {
  const snapshot = structuredClone(existing);
  let generated = 0;
  const result = mergeCategoryImport(
    existing,
    [{ name: "合同", description: "新说明" }, { name: "新类别", description: "" }],
    "cancel",
    () => { generated += 1; return "new-id"; },
  );
  assert.equal(result, null);
  assert.equal(generated, 0);
  assert.deepEqual(existing, snapshot);
});

test("大批量无冲突导入保持顺序并为每个新类别生成 ID", () => {
  const imported = parseCategoryJson(JSON.stringify(Array.from({ length: 157 }, (_, index) => ({
    name: `类别 ${index + 1}`,
    description: `说明 ${index + 1}`,
  }))));
  let nextId = 0;
  const result = mergeCategoryImport(existing, imported, "keep", () => `id-${++nextId}`);
  assert.equal(result.added, 157);
  assert.equal(result.skipped, 0);
  assert.equal(result.overwritten, 0);
  assert.equal(result.labels.length, 159);
  assert.equal(result.labels[2].id, "id-1");
  assert.equal(result.labels[158].name, "类别 157");
});
