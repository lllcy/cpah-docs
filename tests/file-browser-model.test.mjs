import test from "node:test";
import assert from "node:assert/strict";
import { absoluteFilePath, expandedDirectories, flattenEntries, previewSearch } from "../src/file-browser-model.ts";

const file = (relativePath, kind = "file", extra = {}) => ({ relativePath, name: relativePath.split("/").at(-1), kind, availability: "eligible", task: null, ...extra });

test("collapsed ancestors hide their remembered descendants and empty folders stay visible", () => {
  const entries = [file("A", "directory"), file("A/B", "directory"), file("A/B/document.txt"), file("empty", "directory")];
  assert.deepEqual(expandedDirectories(new Set(["A/B"])), [""]);
  const rows = flattenEntries(entries, new Set(["A/B", "empty"]), [{ relativePath: "empty", error: null }], false);
  assert.deepEqual(rows.filter(row => row.entry).map(row => row.path), ["A", "empty"]);
  assert.equal(rows.at(-1).message, "空文件夹");
});

test("search finds a file under a collapsed deep path and retains each ancestor", () => {
  const entries = [file("A", "directory"), file("A/B", "directory"), file("A/B/合同.txt", "file", { task: { status: "failed" } }), file("ignore.txt")];
  const result = previewSearch(entries, "合同", "failed");
  assert.equal(result.matchedCount, 1);
  assert.deepEqual(result.entries.map(entry => entry.relativePath), ["A", "A/B", "A/B/合同.txt"]);
});

test("Windows and macOS paths preserve the configured root", () => {
  assert.equal(absoluteFilePath("C:\\Docs\\", "2026/report.pdf"), "C:\\Docs\\2026\\report.pdf");
  assert.equal(absoluteFilePath("/Users/demo/docs/", "2026/report.pdf"), "/Users/demo/docs/2026/report.pdf");
});

test("a large flat directory keeps numeric filename ordering and every entry", () => {
  const entries = Array.from({ length: 10000 }, (_, index) => file(`样例-${10000 - index}.txt`));
  const rows = flattenEntries(entries, new Set(), [{ relativePath: "", error: null }], false);
  assert.equal(rows.length, 10000);
  assert.equal(rows[0].entry.name, "样例-1.txt");
  assert.equal(rows.at(-1).entry.name, "样例-10000.txt");
});
