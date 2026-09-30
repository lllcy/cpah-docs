# Internationalization

CPAH Docs currently supports English (`en`) and Simplified Chinese (`zh-CN`). Contributions and issue reports may use either language.

## Language selection

On first launch, the frontend reads the preferred system/browser language. Chinese language tags select Simplified Chinese; all other languages select English. Settings offers system, English and Simplified Chinese choices. The explicit choice persists under `cpah-language` in the local webview and updates the document’s `lang` attribute immediately.

The resolved language is sent to `set_ui_language`, saved as `ui-language.json` in the app data directory and used by the native tray/menu-bar menu, copied diagnostic reports and generated index headings. Changing it requests an index rebuild; only managed blocks change. Installer language is independent: Windows bundles support English and Simplified Chinese.

## Frontend messages

`src/i18n.ts` provides `t`, `translate`, `getLocale` and locale-aware number formatting. `src/i18n-react.ts` subscribes React to preference changes. The app subscribes once at its root; components with cached translated calculations must include the active locale in their memo dependencies.

Chinese source messages act as stable keys, following the gettext approach. English translations live in `src/locales/en.json`. Chinese uses the source key directly, so there is no duplicated Chinese catalog to drift out of sync.

```tsx
import { t, formatNumber } from "@/i18n";

<span>{t("{p0} 个待转换", { p0: formatNumber(count) })}</span>
```

Translate full sentences and use named placeholders for values. Preserve each placeholder in every translation so translators can reorder it. Do not concatenate translated sentence fragments. `t` accepts only catalog keys at compile time; `translate` handles predefined metadata labels and known backend guidance, preserving unknown text.

Dates, numbers and file ordering use the resolved locale. Case folding used to validate category identities is locale-independent. The runtime is local and has no translation service or additional network requests.

## Preserve user data

Do not translate file paths, filenames, profile names, custom category names, document content, provider response bodies or persisted protocol identifiers. The historical fallback category `未分类` remains unchanged in YAML and model requests; its display name is “Unclassified” in English. A language switch must never migrate category data.

Native presentation strings use `Language::text` or the shared English catalog in `src-tauri/src/locale.rs`. Raw technical details may remain in the language used by the parser, provider or original backend error. Translate guidance around those details without hiding or rewriting the original diagnostic evidence. Historical Chinese release notes and vendored upstream documentation may remain in their source language.

## Validation

Run `npm test` for locale selection, interpolation, preference persistence, reserved-category compatibility and catalog placeholder checks, then `npm run build`. Rust tests verify switching index headings preserves user sections and document data. CI runs these checks on Windows and macOS.

Before submitting interface changes, inspect all navigation pages in both languages, switch without reloading, reload to verify persistence and check the minimum desktop width (800 px). Include screen-reader labels, placeholders, tooltips, empty states and error guidance.

## Adding another locale

Add a complete catalog with the same keys and placeholders as `en.json`; extend `Locale`, language detection, the settings selector and the native `Language` enum. Update native menu/index/report translations and installer configuration as applicable. Extend the catalog tests for completeness and placeholder parity, and add a language-selection test. Use the language’s own name in the selector.

## 简体中文

首次启动跟随系统语言，其他语言默认使用英文；可在“设置 → 语言”切换并记住选择。中文原文作为稳定翻译键，英文文案集中在 `src/locales/en.json`。新增界面文案请使用 `t`、翻译完整句子并保留参数占位符；日期、数字与文件排序使用当前语言。

语言切换只改变显示文案和索引托管区，不翻译用户文件名、路径、目录名称、自定义类别或正文。历史保留类别 `未分类` 在 YAML 和模型请求中不变，英文界面显示为 Unclassified。修改后运行 `npm test`、`npm run build` 与 Rust 检查，并验证中英文页面和 800 px 窗口。
