# Changelog

**English** | [简体中文](CHANGELOG.zh-CN.md)

Notable changes are recorded here. Versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.4.0] - 2026-10-08

### Added

- English and Simplified Chinese interfaces, automatic system-language detection and a persistent language selector in Settings.
- Localized navigation, help, task states, dialogs, accessibility labels, error guidance, dates, numbers, file sorting, native tray menus and managed knowledge-index headings.
- English project documentation with retained Chinese versions, bilingual issue/PR templates and a translation contribution guide.
- English and Simplified Chinese Windows installer configuration, and internationalization checks in CI and release scripts.

### Compatibility

- User filenames, folder names, custom categories and document content are preserved when switching languages. The reserved `未分类` YAML value remains unchanged and displays as “Unclassified” in English.

## [1.3.0] - 2026-09-29

### Added and improved

- Office conversion now uses anydoc 0.2.4. Legacy DOC/PPT convert locally, with support for macro-enabled Office, OpenDocument, EPUB and RTF and readable embedded images.
- HTML/HTM/TXT retain AnyToMD processing; Markdown is copied unchanged.
- PDFs extract locally first and use MinerU only when OCR is explicitly required. Large-PDF splitting, recovery and retries remain supported; OCR decisions persist with the source version.
- Updated format settings, engine labels and help while preserving existing task compatibility.
- JSON category import with batch validation, description-conflict preview and keep/replace/cancel choices. Rules save without starting historical classification.

### Fixed

- Removing a watch folder cancels conversion, classification and indexing, waits for active writes and cleans task records and MinerU cache while keeping source and output files.
- Stale windows and file actions cannot restore or requeue deleted folders. Late decision-model responses and MinerU extraction respect cancellation.
- Release scripts support custom Cargo build directories and run category-import tests.

## [1.2.0] - 2026-09-28

### Added

- System One-compatible decision models alongside language models. Decision mode sends at most the first 32 KiB of Markdown.
- Complete source tree, filename search, conversion-status filtering and details for untracked files and empty folders.
- Per-file conversion/classification actions that prioritize the selected file and resume the corresponding queue.

### Improved and fixed

- Persistent collapsed navigation and a transparent purple/blue app icon.
- Saved credential placeholders for tokens and API keys.
- Dismissible runtime errors that do not reopen on identical polling responses.
- Priority queues release after failures, missing source files or folder changes; classification checks current conversion output and source state.
- Updated rustls to address RUSTSEC-2026-0285.

## [1.1.3] - 2026-08-16

### Fixed

- Enabled the cargo-about CLI during installation in both release workflows.
- Release scripts use .NET SHA-256 on Windows for compatibility with GitHub runners.

## [1.1.2] - 2026-08-16

### Security

- Reject output-path escapes through parent traversal, symlinks and Windows junctions.
- Require HTTPS for remote model connections, allow HTTP only for loopback and disable redirects.
- Add timeouts to connections, requests, classification runs and tool-calling probes.

### Fixed

- Keep output when sources are offline, inaccessible or disabled; deletion synchronization avoids active conversions.
- Describe the actual output-folder `.trash` deletion policy.
- Include Windows and macOS dependencies in third-party license notices distributed with both platforms.

## [1.1.1] - 2026-08-15

### Added

- macOS support and a universal DMG for Apple Silicon and Intel.
- CI and tag releases verify and build Windows and macOS artifacts.
- Local mock-provider regression tests for tool calling and YAML updates.

### Fixed

- Removed hard-coded Windows native frontend dependencies so macOS can run `npm ci`.
- Legacy XLS image extraction no longer treats XLS as OOXML ZIP.
- Cross-platform credential and path guidance.

## [1.1.0] - 2026-08-14

### Added

- Recursive synchronization of multiple source/output folder pairs.
- Local Office/text conversion and MinerU cloud parsing.
- Independent watching, conversion and classification queues.
- Single/multiple candidate categories written to `cpah_categories`.
- Root and nested knowledge indexes with folder/category navigation.
- Health checks, redacted diagnostics, rotating logs and recovery after unexpected exits.
- Windows standalone EXE release scripts and SHA-256 checksums.

[1.1.0]: https://github.com/lllcy/cpah-docs/releases/tag/v1.1.0
[1.1.1]: https://github.com/lllcy/cpah-docs/releases/tag/v1.1.1
[1.1.2]: https://github.com/lllcy/cpah-docs/releases/tag/v1.1.2
[1.1.3]: https://github.com/lllcy/cpah-docs/releases/tag/v1.1.3
[1.2.0]: https://github.com/lllcy/cpah-docs/releases/tag/v1.2.0
[1.3.0]: https://github.com/lllcy/cpah-docs/releases/tag/v1.3.0
[1.4.0]: https://github.com/lllcy/cpah-docs/releases/tag/v1.4.0
