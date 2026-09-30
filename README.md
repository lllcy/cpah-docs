# CPAH Docs

[![CI](https://github.com/lllcy/cpah-docs/actions/workflows/ci.yml/badge.svg)](https://github.com/lllcy/cpah-docs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/lllcy/cpah-docs?display_name=tag)](https://github.com/lllcy/cpah-docs/releases)

**English** | [简体中文](README.zh-CN.md)

A desktop app for **Windows and macOS** that watches folders, converts documents to Markdown and builds local knowledge indexes. Optional model-based classification selects your categories and writes them to Markdown frontmatter.

The interface supports **English and Simplified Chinese**. It follows your system language on first launch, uses English for other languages, and remembers your choice in **Settings → Language**.

## Features

- **Watch multiple folders.** Mirror each source folder into a separate output folder, preserving subfolders, including empty ones. Changes and deletions are synchronized.
- **Control each queue independently.** Watching discovers and queues files; conversion and classification start separately. New installations watch automatically and wait for you to start conversion.
- **Convert locally.** Office, legacy DOC/XLS/PPT, macro-enabled Office, OpenDocument, EPUB, RTF and CSV use the vendored anydoc 0.2.4 converter. HTML/HTM/TXT use AnyToMD. Markdown is copied byte for byte.
- **Use OCR when needed.** PDFs are extracted locally first. Only an explicit “OCR required” result sends a PDF to MinerU. Damaged, encrypted or oversized PDFs are not uploaded as a fallback. Images use MinerU.
- **Process large PDFs.** OCR documents over 200 pages are submitted in page ranges; files exceeding MinerU’s 200 MB limit are split losslessly. Source files up to 512 MiB are supported. Parts resume independently and merge into one Markdown file.
- **Classify with your own categories.** Choose single or multiple categories, import category definitions from JSON, and inspect model calls, read coverage and token usage. Both OpenAI-compatible language models and System One-compatible decision models are supported.
- **Browse the complete source tree.** Search filenames recursively, filter by conversion status and inspect individual files, including unsupported formats and empty folders. Prioritize conversion or classification of a selected file.
- **Recover work after restart.** SQLite persists task state and MinerU polling. Offline health checks and redacted diagnostic reports help troubleshoot failures.
- **Keep credentials in the OS credential store.** Tokens and API keys use Windows Credential Manager or macOS Keychain. Closing the window keeps the app in the system tray or menu bar.

Office formats can export readable embedded images to `.assets`. Local PDF conversion preserves extracted text, tables and image placeholders, but does not export PDF image attachments. See the vendored converter’s [provenance and patches](src-tauri/vendor/anydoc/UPSTREAM.md).

## Download

Download from [GitHub Releases](https://github.com/lllcy/cpah-docs/releases). End users do not need Python, Node.js or Rust.

| Platform | Artifact | Requirement |
| --- | --- | --- |
| Windows x64 | `CPAH-Docs-v<version>-windows-x64.exe` | Microsoft Edge WebView2 Runtime, usually available on modern Windows 10/11 |
| macOS, Apple Silicon and Intel | `CPAH-Docs-v<version>-macos-universal.dmg` | A supported macOS environment |

Releases currently have no commercial Windows signing certificate or Apple Developer signing/notarization. Windows SmartScreen may show an unknown publisher. On macOS, use Finder’s **Open** action or confirm opening in **System Settings → Privacy & Security** if Gatekeeper prompts. Download from this repository and verify integrity using the release’s `SHA256SUMS.txt`.

## Quick start

1. Create a source folder for Word, PDF, Excel, PowerPoint and other original documents.
2. Create a separate output folder for Markdown and assets. The folders must be distinct, must not contain each other and must not overlap other configured folders.
3. Add both paths in **Watch folders** and save. Watching scans existing files and queues them.
4. Choose the extensions to process on the **Formats** page.
5. For images or PDFs requiring OCR, save a MinerU token in **Settings**. Office documents and text PDFs do not require a token.
6. Review the queue and click **Start conversion** on the **Conversions** page.
7. Optionally configure and test a classification model in **Settings**, enable classification in the folder’s settings, and define candidate categories. Then click **Start classification** on the **Classification** page.

Each folder’s **Enabled** switch controls watching, conversion, classification and indexing for that folder. Format switches determine which files enter the conversion queue. Existing installations retain their saved queue states after upgrading.

Browse files under **Watch folders → Files**. Searching and refreshing do not enqueue tasks or call a model. A file’s conversion/classification action resumes the corresponding queue, prioritizes that file and continues other tasks afterward. Active tasks are not interrupted. Classification requires current, completed conversion output and configured folder rules and model settings.

Clicking the window’s close button hides it. Use **Quit** from the tray/menu-bar icon to exit completely.

## Output and indexes

```text
Source/reports/example.pdf
Output/reports/example.md
Output/reports/example.assets/
Output/index.md
Output/reports/index.md
```

For same-stem source files with different formats, output names retain the source extension, such as `example.pdf.md` and `example.docx.md`, to prevent overwriting.

The output root and each subfolder containing Markdown maintain `index.md`, with breadcrumbs, folder navigation, category views, pending classification and recent updates. Indexing scans local paths and YAML; it uses no model calls or tokens. Generated indexes do not enter the classification queue.

Existing `index.md` content is preserved. Only the managed block is updated:

```markdown
<!-- cpah:index:start -->
Generated folder and category index
<!-- cpah:index:end -->
```

Language changes update managed index headings while preserving filenames, folder names, category names, document content and user-written index sections. Empty generated indexes are removed when their documents disappear; indexes containing user text retain that text.

Example classification frontmatter:

```yaml
---
source: example.pdf
converter: mineru
cpah_categories:
  - Training
  - Audit
---
```

Other valid YAML fields, comments, document body, UTF-8 BOM and newline style are preserved. Invalid YAML or files modified during classification are not overwritten. Markdown is copied unchanged until optional classification updates its categories.

For compatibility, the built-in fallback category remains the literal **`未分类`** in YAML and model responses. Its English display name is **Unclassified**. Switching the interface language does not migrate this value or translate your custom categories.

## Import categories

In a folder’s settings, select **Import JSON** beside **Document classification** and paste a plain, nonempty JSON array:

```json
[
  {
    "name": "Invoices",
    "description": "Invoices and their supporting records."
  }
]
```

Each item must contain only `name` and `description`. Names must be nonempty and unique (ignoring case and surrounding whitespace); `未分类` is reserved. Descriptions must be strings and may be empty. Markdown fences, IDs, selection modes and the legacy `value` field are not accepted.

The entire import is validated before changes are applied. New categories are appended; identical entries are skipped. For description conflicts, keep all current descriptions, replace them all or cancel. Importing saves the rules without starting classification or rerunning old tasks.

## Classification models

| Type | Protocol and behavior |
| --- | --- |
| Language model (default) | OpenAI-compatible Chat Completions with tool calling. The model reads document chunks and submits categories using the provided tools. |
| Decision model | System One-compatible service. Jev example: `https://api.typesafe.ai/v1`, model `jev-latest`. Alibaba Bailian example: `https://<workspace-id>.cn-beijing.maas.aliyuncs.com/compatible-mode/v1`, model `decision-model-preview`. Complete `/systemone` URLs are also accepted. |

One model connection is saved at a time. Leaving an API key blank keeps the existing key. Saving or testing a connection does not start stopped classification queues. Old configurations without a model type use the language-model mode.

Decision models receive at most the first 32 KiB of Markdown, excluding existing `cpah_categories`; longer documents are marked truncated. Single-category mode supports up to 254 custom categories plus the fallback. Multiple-category mode uses one `noul` question per category, up to 16 per batch, and selects matches above 0.5 probability. This threshold is a selection rule, not an accuracy guarantee. Files are updated only after all batches succeed and responses pass validation.

## Privacy and data

- There is no telemetry, advertising or background update checking.
- Local conversion does not upload documents.
- MinerU parsing uploads documents to your configured MinerU service.
- Classification sends Markdown to your configured model service.
- Tokens and API keys are stored in the OS credential store, never in `settings.json`.
- Directory symlinks are not followed. Remote transfers and archives have size and entry limits and reject path traversal.
- Settings and document writes use atomic replacement. Damaged settings can recover from the last valid backup.
- Release logs contain no credentials or document body. They rotate at 2 MiB and retain three files.

Settings, backups, the SQLite database and logs live in Tauri’s app data directory for `com.cpah.docs`: the Windows user app data folder or `~/Library/Application Support/com.cpah.docs` on macOS. The interface preference is stored in the local webview; its resolved language is mirrored to `ui-language.json` for native menus and indexes. Early internal-version data and credentials migrate on first upgrade.

Generated Markdown, indexes and assets are written only to each configured output folder. Review your chosen cloud services’ terms, data processing and costs before enabling them. CPAH Docs is not affiliated with MinerU, OpenAI or other model providers.

## Development

Prerequisites: **Node.js 24.15+**, **Rust 1.97+**, and platform build tools. On Windows, install Visual Studio’s **Desktop development with C++**, including MSVC x64/x86 and the Windows SDK. On macOS, install Xcode Command Line Tools or Xcode.

```shell
npm ci
npm run tauri dev
```

Frontend-only preview with synthetic data:

```shell
npm run dev
# Open http://localhost:1420/?preview
```

Validation:

```shell
npm run build
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Run `npm run tauri build` for a production build, or `npm run release` for release checks, third-party license verification, platform artifacts and SHA-256 checksums. Publishing a matching `v*` tag builds Windows x64 and macOS universal artifacts; both must succeed before the GitHub Release is created. Releases include `LICENSE.txt`, `THIRD_PARTY_LICENSES.md` and `SHA256SUMS.txt`.

Real-service regression tests are ignored by default. MinerU tests use `CPAHDOCS_MINERU_E2E` and `CPAHDOCS_MINERU_TOKEN` (or a saved token). Large-PDF tests use `CPAHDOCS_MINERU_PAGE_RANGES_E2E` or `CPAHDOCS_MINERU_PHYSICAL_E2E`. Tool-calling tests use `CPAHDOCS_AGENT_BASE_URL`, `CPAHDOCS_AGENT_MODEL` and `CPAHDOCS_AGENT_API_KEY`. Keep all real fixtures in Git-ignored local folders.

To regenerate licenses, run `node scripts/generate-third-party-licenses.mjs` with cargo-about 0.9.1. The app icon comes from `app-icon.png`; generate platform resources with `npm run tauri icon app-icon.png` and copy `128x128@2x.png` to `public/app-icon.png`.

## Contributing and licenses

Contributions in English or Chinese are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md), the [Code of Conduct](CODE_OF_CONDUCT.md) and the [translation guide](docs/internationalization.md). Changes are checked on Windows and macOS in CI; dependency security checks run weekly.

Original code is licensed under [MIT](LICENSE). See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) for third-party notices. Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).
