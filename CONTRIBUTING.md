# 参与贡献

感谢你愿意改进 CPAH Docs。提交代码前，请先搜索现有 Issue，较大的功能建议先开 Issue 对齐范围。

## 本地开发

需要 Node.js 24.15+ 和 Rust 1.97+。Windows 10/11 还需要 Visual Studio Installer 中的“使用 C++ 的桌面开发”、MSVC x64/x86 和 Windows SDK；macOS 需要 Xcode Command Line Tools 或完整 Xcode。

```shell
npm ci
npm run tauri dev
```

提交前请运行：

```shell
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

## Pull Request

- 一个 PR 聚焦一个问题，说明用户场景、改动和验证方式。
- 行为变化应补充测试；界面变化请附脱敏截图。
- 不要提交真实文档、任务数据库、运行日志、Token、API Key、证书或个人完整路径。
- 不要把 `debug-ai.json`、`.env` 或本机 E2E 资料加入 Git。
- 用户可见行为变化请同步更新 README 或帮助页。

提交信息推荐使用简洁的 Conventional Commits，例如 `fix: resume MinerU polling after restart`。

## 报告安全问题

安全漏洞不要发布到公开 Issue，请按照 [SECURITY.md](SECURITY.md) 私下报告。
