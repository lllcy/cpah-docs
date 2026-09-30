# Contributing to CPAH Docs

**English** | [简体中文](CONTRIBUTING.zh-CN.md)

Contributions in English or Chinese are welcome: bug reports, documentation, translations and code. Search existing issues first. For substantial changes, open an issue to discuss the scope before implementing them.

## Local development

Use Node.js 24.15+ and Rust 1.97+. Windows requires Visual Studio’s Desktop development with C++, MSVC x64/x86 and the Windows SDK. macOS requires Xcode Command Line Tools or Xcode.

```shell
npm ci
npm run tauri dev
```

Use `npm run dev` and open `http://localhost:1420/?preview` to inspect the frontend with synthetic data, without accessing personal documents or model services.

Before submitting a pull request:

```shell
npm run build
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

## Pull requests

- Focus each PR on one problem and explain the user scenario, resulting behavior and validation.
- Add meaningful tests for behavior changes. Include redacted screenshots for interface changes, covering both supported languages when applicable.
- Do not commit real documents, databases, logs, tokens, API keys, certificates or full personal paths. Keep `.env`, `debug-ai.json` and real E2E fixtures out of Git.
- Update the English and Chinese README or in-app help when user-visible behavior changes.
- Use translation catalog entries for interface messages. Keep filenames, custom category names, database identifiers and persisted YAML values independent of the interface language. See [the translation guide](docs/internationalization.md).

Concise Conventional Commit messages are recommended, for example `fix: resume MinerU polling after restart`.

Please follow the [Code of Conduct](CODE_OF_CONDUCT.md). Report security vulnerabilities privately through [SECURITY.md](SECURITY.md), rather than public issues.
