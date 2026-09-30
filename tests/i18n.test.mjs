import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import english from "../src/locales/en.json" with { type: "json" };
import { getLocale, getLanguagePreference, resolveLocale, setLanguagePreference, subscribeLanguage, t, translate, translateHealthTitle, formatNumber } from "../src/i18n.ts";
import { parseCategoryJson } from "../src/tag-import.ts";
import { compareEntries, flattenEntries } from "../src/file-browser-model.ts";

test("language detection respects the preferred language and explicit overrides", () => {
  for (const language of ["zh-CN", "zh-TW", "zh-Hans", "ZH-hk"]) assert.equal(resolveLocale("system", [language]), "zh-CN");
  for (const language of ["en-US", "en-GB", "de-DE", "ja-JP", "fr-FR"]) assert.equal(resolveLocale("system", [language, "zh-CN"]), "en");
  assert.equal(resolveLocale("system", []), "en");
  assert.equal(resolveLocale("en", ["zh-CN"]), "en");
  assert.equal(resolveLocale("zh-CN", ["en-US"]), "zh-CN");
});

test("all translations are nonempty and preserve placeholder names and counts", () => {
  const placeholders = (text) => [...text.matchAll(/\{(\w+)\}/g)].map(match => match[1]).sort();
  for (const [key, value] of Object.entries(english)) {
    assert.ok(value.trim(), `Empty translation: ${key}`);
    assert.deepEqual(placeholders(value), placeholders(key), `Placeholder mismatch: ${key}`);
  }
});

test("switching updates messages while preserving unknown user text", () => {
  let updates = 0;
  const unsubscribe = subscribeLanguage(() => updates++);
  setLanguagePreference("en");
  assert.equal(t("第 {p0}–{p1} 页", { p0: 1, p1: 200 }), "Pages 1–200");
  assert.equal(translate("待执行"), "Queued");
  assert.equal(translate("我的合同.pdf"), "我的合同.pdf");
  assert.equal(translateHealthTitle("项目 · 监控目录 · 输出目录"), "项目 · 监控目录 · Output folder");
  assert.equal(formatNumber(12345), "12,345");
  assert.equal(t("已重新提交「{p0}」。", { p0: "{p1}.pdf" }), "Resubmitted “{p1}.pdf”.");
  setLanguagePreference("zh-CN");
  assert.equal(translateHealthTitle("项目 · 监控目录 · 输出目录"), "项目 · 监控目录 · 输出目录");
  assert.equal(t("第 {p0}–{p1} 页", { p0: 1, p1: 200 }), "第 1–200 页");
  assert.equal(updates, 2);
  unsubscribe();
  setLanguagePreference("en");
  assert.equal(updates, 2);
});

test("preferences persist and restore without requiring writable storage", async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  const values = new Map();
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
    getItem: key => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  } });
  try {
    setLanguagePreference("zh-CN");
    const restored = await import("../src/i18n.ts?restore-test");
    assert.equal(restored.getLanguagePreference(), "zh-CN");
    assert.equal(restored.getLocale(), "zh-CN");
    values.set("cpah-language", "invalid-locale");
    const invalid = await import("../src/i18n.ts?invalid-test");
    assert.equal(invalid.getLanguagePreference(), "system");
    Object.defineProperty(globalThis, "localStorage", { configurable: true, get() { throw new Error("Storage blocked"); } });
    assert.doesNotThrow(() => setLanguagePreference("en"));
    assert.equal(getLocale(), "en");
    assert.equal(getLanguagePreference(), "en");
    const unavailable = await import("../src/i18n.ts?unavailable-test");
    assert.equal(unavailable.getLanguagePreference(), "system");
  } finally {
    if (original) Object.defineProperty(globalThis, "localStorage", original);
    else delete globalThis.localStorage;
    setLanguagePreference("en");
  }
});

test("English import errors retain the reserved category and preserve custom names", () => {
  setLanguagePreference("en");
  assert.throws(() => parseCategoryJson('[{"name":"未分类","description":""}]'), /reserved category/);
  assert.throws(() => parseCategoryJson('[{"name":"A","description":""},{"name":" a ","description":""}]'), /Item 2.*duplicate/);
  assert.throws(() => parseCategoryJson("{}"), /nonempty array/);
  assert.deepEqual(parseCategoryJson('[{"name":"审计资料","description":"中文说明"}]'), [{ name: "审计资料", description: "中文说明" }]);
});

test("empty-folder messages change language and numeric file ordering stays correct", () => {
  const entry = { kind: "directory", name: "empty", relativePath: "empty" };
  const rows = () => flattenEntries([entry], new Set(["empty"]), [{ relativePath: "empty", error: null }], false);
  setLanguagePreference("en");
  assert.equal(rows().at(-1).message, "Empty folder");
  setLanguagePreference("zh-CN");
  assert.equal(rows().at(-1).message, "空文件夹");
  for (const language of ["en", "zh-CN"]) {
    setLanguagePreference(language);
    assert.ok(compareEntries({ kind: "file", name: "report-2", relativePath: "report-2" }, { kind: "file", name: "report-10", relativePath: "report-10" }) < 0);
  }
});

test("Windows installers offer both supported languages", () => {
  const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  assert.deepEqual(config.bundle.windows.wix.language, ["en-US", "zh-CN"]);
  assert.deepEqual(config.bundle.windows.nsis.languages, ["English", "SimpChinese"]);
  assert.equal(config.bundle.windows.nsis.displayLanguageSelector, true);
});
